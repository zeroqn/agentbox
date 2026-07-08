//! Host-side libkrun helper command construction and process spawning.

use anyhow::{Context, Result, bail};
use std::ffi::OsString;
use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use crate::logging::INTERNAL_LOG_LEVEL_ENV;
use crate::runtime::launch::config::LaunchConfig;
use crate::runtime::seccomp;
use crate::runtime::session::attach::{self, AttachInputPolicy, AttachOutcome};
use crate::runtime::session::profile::{LOFTD_HOST_PROFILE_ENV, LoftdHostProfiler};
use crate::runtime::session::supervisor::identity::KeepIdLauncher;
use crate::runtime::session::supervisor::managed_exit_marker;
use crate::runtime::session::supervisor::readiness_pipe::{ParentReadyPipe, READY_FD_ENV};
use crate::runtime::session::supervisor::sigwinch::SigwinchForwarder;
use crate::runtime::session::supervisor::{ChildStatus, LIBKRUN_ENTER_HELPER_ARG};
use crate::runtime::session::task_control::{
    ActiveTaskSpec, ProcessIdentity, remove_active_task, write_active_task,
};

pub(crate) fn run_helper_process(
    config: &LaunchConfig,
    config_path: &Path,
    profiler: &mut LoftdHostProfiler,
    active_task: &ActiveTaskSpec,
    daemon_initial_attach: bool,
    attach_input_policy: AttachInputPolicy,
) -> Result<ChildStatus> {
    let host_profile_enabled = profiler.is_enabled();
    let mut ready_pipe = if config.managed_session.is_some() {
        Some(ParentReadyPipe::create()?)
    } else {
        None
    };
    let ready_fd = ready_pipe.as_ref().and_then(ParentReadyPipe::writer_fd);
    let spec = profiler.measure_result("helper_command_build", || {
        let executable = std::env::current_exe()
            .context("failed to resolve loftd executable for libkrun helper")?;
        build_helper_command(
            &executable,
            config_path,
            config,
            host_profile_enabled,
            ready_fd,
        )
    })?;
    tracing::debug!(program = ?spec.program, args = ?spec.args, log_level = config.log_level.as_str(), "loftd libkrun helper command constructed");
    let audit_trace_path = config.seccomp.audit_trace_path().map(Path::to_path_buf);
    let mut command = spec.into_command();
    let managed_helper_stderr = if config.managed_session.is_some() {
        managed_exit_marker::reset_observed_guest_exit(&active_task.task_dir)?;
        Some(ManagedHelperStderr::create(&active_task.task_dir)?)
    } else {
        None
    };
    if config.managed_session.is_some() {
        command.stdin(Stdio::null()).stdout(Stdio::null()).stderr(
            managed_helper_stderr
                .as_ref()
                .expect("managed helper stderr must be initialized")
                .spawn_stdio()?,
        );
    } else {
        command
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());
    }
    // SAFETY: this pre-exec hook only calls async-signal-safe libc setsid in
    // the child process before exec. A dedicated session and process group give
    // loftd a bounded task-level control identity for `loftd kill`.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() >= 0 {
                Ok(())
            } else {
                Err(std::io::Error::last_os_error())
            }
        });
    }
    let mut child = profiler.measure_result("helper_spawn_process", || {
        command.spawn().with_context(|| {
        format!(
            "failed to start loftd libkrun helper for '{}'; rootless direct-libkrun launches require util-linux unshare plus newuidmap/newgidmap and usable /etc/subuid + /etc/subgid entries",
            config_path.display()
        )
        })
    })?;
    if let Some(pipe) = &mut ready_pipe {
        pipe.close_parent_writer();
    }
    tracing::debug!(pid = child.id(), "loftd libkrun helper spawned");
    let process = match ProcessIdentity::from_spawned_process(child.id(), child.id(), child.id()) {
        Ok(process) => process,
        Err(err) => {
            terminate_spawned_child_group(&mut child);
            let _ = remove_active_task(&active_task.task_dir);
            return Err(err).context("failed to identify active loftd task after helper spawn");
        }
    };
    let _sigwinch_forwarder = if config.managed_session.is_some() {
        None
    } else {
        match start_sigwinch_forwarder(process.pgid) {
            Ok(forwarder) => forwarder,
            Err(err) => {
                terminate_spawned_child_group(&mut child);
                let _ = remove_active_task(&active_task.task_dir);
                return Err(err)
                    .context("failed to start loftd helper SIGWINCH forwarder after helper spawn");
            }
        }
    };
    if let Err(err) = write_active_task(active_task.clone(), process) {
        terminate_spawned_child_group(&mut child);
        let _ = remove_active_task(&active_task.task_dir);
        return Err(err).context("failed to record active loftd task after helper spawn");
    }
    if let Some(managed) = &config.managed_session {
        let ready_pipe = ready_pipe.as_mut().ok_or_else(|| {
            anyhow::anyhow!("managed loftd helper readiness pipe was not initialized")
        })?;
        if let Err(err) = profiler.measure_result("helper_managed_attach_ready", || {
            ready_pipe.wait_for_ready(&mut child, Duration::from_secs(35))
        }) {
            terminate_spawned_child_group(&mut child);
            let _ = remove_active_task(&active_task.task_dir);
            replay_managed_helper_stderr(managed_helper_stderr.as_ref());
            return Err(context_audit_error(
                err,
                audit_trace_path.as_deref(),
                "failed while waiting for managed loftd attach readiness",
            ));
        }
        let exit_observer =
            |code| managed_exit_marker::write_observed_guest_exit(&active_task.task_dir, code);
        let attach_result = profiler.measure_result("helper_initial_attach", || {
            attach::attach_to_ready_socket_with_input_policy_and_exit_observer(
                &managed.attach_socket,
                daemon_initial_attach,
                attach_input_policy,
                Some(&exit_observer),
            )
        });
        return match attach_result {
            Ok(AttachOutcome::Detached) => Ok(ChildStatus::detached()),
            Ok(AttachOutcome::Exited(code)) => {
                let status = match child.wait() {
                    Ok(status) => status,
                    Err(err) => {
                        replay_managed_helper_stderr(managed_helper_stderr.as_ref());
                        return Err(err)
                            .context("failed to wait for managed loftd helper after guest exit");
                    }
                };
                managed_helper_exit_result(
                    status,
                    code,
                    &active_task.task_dir,
                    managed_helper_stderr.as_ref(),
                )
            }
            Err(err) => {
                terminate_spawned_child_group(&mut child);
                let _ = remove_active_task(&active_task.task_dir);
                replay_managed_helper_stderr(managed_helper_stderr.as_ref());
                Err(context_audit_error(
                    err,
                    audit_trace_path.as_deref(),
                    "failed to attach to managed loftd guest session",
                ))
            }
        };
    }

    let status = profiler.measure_result("helper_wait_process", || {
        child
            .wait()
            .context("failed to wait for loftd libkrun helper")
    })?;
    tracing::debug!(?status, "loftd libkrun helper exited");
    Ok(match status.code() {
        Some(code) => ChildStatus::exited(code),
        None => ChildStatus::signaled(),
    })
}

const MANAGED_HELPER_STDERR_LOG: &str = "helper.stderr.log";

#[derive(Debug, Clone)]
struct ManagedHelperStderr {
    path: PathBuf,
}

impl ManagedHelperStderr {
    fn create(task_state_dir: &Path) -> Result<Self> {
        let path = task_state_dir.join(MANAGED_HELPER_STDERR_LOG);
        OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&path)
            .with_context(|| {
                format!(
                    "failed to create managed loftd helper stderr log '{}'",
                    path.display()
                )
            })?;
        Ok(Self { path })
    }

    fn spawn_stdio(&self) -> Result<Stdio> {
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .with_context(|| {
                format!(
                    "failed to open managed loftd helper stderr log '{}' for helper",
                    self.path.display()
                )
            })?;
        Ok(Stdio::from(file))
    }

    fn read_to_string(&self) -> Result<String> {
        let mut file = OpenOptions::new()
            .read(true)
            .open(&self.path)
            .with_context(|| {
                format!(
                    "failed to read managed loftd helper stderr log '{}'",
                    self.path.display()
                )
            })?;
        let mut contents = String::new();
        file.read_to_string(&mut contents)?;
        Ok(contents)
    }
}

fn managed_helper_exit_result(
    status: ExitStatus,
    guest_code: i32,
    task_state_dir: &Path,
    stderr_log: Option<&ManagedHelperStderr>,
) -> Result<ChildStatus> {
    managed_helper_exit_result_with_replay(status, guest_code, task_state_dir, stderr_log, true)
}

fn managed_helper_exit_result_with_replay(
    status: ExitStatus,
    guest_code: i32,
    task_state_dir: &Path,
    stderr_log: Option<&ManagedHelperStderr>,
    replay_stderr: bool,
) -> Result<ChildStatus> {
    tracing::debug!(
        ?status,
        guest_code,
        "managed loftd helper exited after guest exit"
    );
    if status.success() {
        return Ok(ChildStatus::exited(guest_code));
    }
    let stderr = match stderr_log
        .map(ManagedHelperStderr::read_to_string)
        .transpose()
    {
        Ok(Some(contents)) => contents,
        Ok(None) => String::new(),
        Err(err) => format!("failed to read managed helper stderr log: {err:#}\n"),
    };
    let observation =
        managed_exit_marker::read_matching_observed_guest_exit(task_state_dir, guest_code);
    if matches!(
        observation,
        managed_exit_marker::ManagedExitObservation::ObservedGuestExit(_)
    ) && managed_helper_status_is_marker_explained(&status, guest_code, &stderr)
    {
        return Ok(ChildStatus::exited(guest_code));
    }
    if replay_stderr {
        replay_stderr_text(stderr.as_bytes());
    }
    match status.code() {
        Some(code) => bail!(
            "managed loftd helper exited with status {code} after guest exited with status {guest_code}"
        ),
        None => {
            bail!("managed loftd helper was terminated after guest exited with status {guest_code}")
        }
    }
}

fn managed_helper_status_is_marker_explained(
    status: &ExitStatus,
    guest_code: i32,
    stderr: &str,
) -> bool {
    if status.code() != Some(1) {
        return false;
    }
    let mut saw_duplicate_worker_failure = false;
    let mut saw_duplicate_helper_failure = false;
    let expected_worker_line = format!(
        "loftd internal VM worker: sandboxed loftd VM worker child exited with status {guest_code}"
    );
    for line in stderr
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        if line == expected_worker_line {
            saw_duplicate_worker_failure = true;
            continue;
        }
        if line == "loftd internal: loftd VM worker exited with status 1" {
            saw_duplicate_helper_failure = true;
            continue;
        }
        return false;
    }
    saw_duplicate_worker_failure && saw_duplicate_helper_failure
}

fn replay_managed_helper_stderr(stderr_log: Option<&ManagedHelperStderr>) {
    let Some(stderr_log) = stderr_log else {
        return;
    };
    match stderr_log.read_to_string() {
        Ok(contents) => replay_stderr_text(contents.as_bytes()),
        Err(err) => replay_stderr_text(
            format!("failed to read managed helper stderr log: {err:#}\n").as_bytes(),
        ),
    }
}

fn replay_stderr_text(contents: &[u8]) {
    if contents.is_empty() {
        return;
    }
    let mut stderr = std::io::stderr().lock();
    let _ = write_normalized_stderr(&mut stderr, contents);
}

fn write_normalized_stderr<W: Write>(stderr: &mut W, contents: &[u8]) -> std::io::Result<()> {
    if contents.is_empty() {
        return Ok(());
    }
    stderr.write_all(b"\r\n")?;
    stderr.write_all(contents)?;
    if !contents.ends_with(b"\n") {
        stderr.write_all(b"\n")?;
    }
    stderr.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::process::ExitStatusExt;

    #[test]
    fn normalized_stderr_replay_starts_on_fresh_terminal_line() {
        let mut output = Vec::new();

        write_normalized_stderr(&mut output, b"loftd internal: failed").expect("normalized write");

        assert_eq!(output, b"\r\nloftd internal: failed\n");
    }

    #[test]
    fn managed_helper_stderr_sink_is_file_backed_for_detached_lifecycle() {
        let temp = tempfile::tempdir().expect("tempdir");
        let log = ManagedHelperStderr::create(temp.path()).expect("stderr log");

        let status = Command::new("sh")
            .arg("-c")
            .arg("printf 'detached helper diagnostic\\n' >&2")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(log.spawn_stdio().expect("spawn stdio"))
            .status()
            .expect("run stderr writer");

        assert!(status.success());

        assert_eq!(
            log.read_to_string().expect("read log"),
            "detached helper diagnostic\n"
        );
        assert!(
            temp.path().join(MANAGED_HELPER_STDERR_LOG).exists(),
            "detached/preserved task state should retain helper stderr log"
        );
    }

    #[test]
    fn managed_helper_status_accepts_marker_explained_duplicate_status() {
        let temp = tempfile::tempdir().expect("tempdir");
        managed_exit_marker::write_observed_guest_exit(temp.path(), 130).expect("marker");
        let log = ManagedHelperStderr::create(temp.path()).expect("stderr log");
        std::fs::write(
            &log.path,
            "loftd internal VM worker: sandboxed loftd VM worker child exited with status 130\n\
             loftd internal: loftd VM worker exited with status 1\n",
        )
        .expect("duplicate stderr text");

        let result = managed_helper_exit_result_with_replay(
            ExitStatus::from_raw(1 << 8),
            130,
            temp.path(),
            Some(&log),
            false,
        )
        .expect("matching marker should explain duplicate helper status");

        assert_eq!(result, ChildStatus::exited(130));
    }

    #[test]
    fn managed_helper_status_rejects_empty_stderr_despite_marker() {
        let temp = tempfile::tempdir().expect("tempdir");
        managed_exit_marker::write_observed_guest_exit(temp.path(), 130).expect("marker");
        let log = ManagedHelperStderr::create(temp.path()).expect("stderr log");

        let err = managed_helper_exit_result_with_replay(
            ExitStatus::from_raw(1 << 8),
            130,
            temp.path(),
            Some(&log),
            false,
        )
        .expect_err("empty stderr must not be classified as known duplicate noise");

        assert!(format!("{err:#}").contains("managed loftd helper exited with status 1"));
    }

    #[test]
    fn managed_helper_status_rejects_mismatched_duplicate_worker_status() {
        let temp = tempfile::tempdir().expect("tempdir");
        managed_exit_marker::write_observed_guest_exit(temp.path(), 130).expect("marker");
        let log = ManagedHelperStderr::create(temp.path()).expect("stderr log");
        std::fs::write(
            &log.path,
            "loftd internal VM worker: sandboxed loftd VM worker child exited with status 129\n\
             loftd internal: loftd VM worker exited with status 1\n",
        )
        .expect("duplicate stderr text");

        let err = managed_helper_exit_result_with_replay(
            ExitStatus::from_raw(1 << 8),
            130,
            temp.path(),
            Some(&log),
            false,
        )
        .expect_err("different worker status must not be classified as known duplicate noise");

        assert!(format!("{err:#}").contains("managed loftd helper exited with status 1"));
    }

    #[test]
    fn managed_helper_status_rejects_duplicate_worker_line_with_extra_text() {
        let temp = tempfile::tempdir().expect("tempdir");
        managed_exit_marker::write_observed_guest_exit(temp.path(), 130).expect("marker");
        let log = ManagedHelperStderr::create(temp.path()).expect("stderr log");
        std::fs::write(
            &log.path,
            "loftd internal VM worker: sandboxed loftd VM worker child exited with status 130: extra\n\
             loftd internal: loftd VM worker exited with status 1\n",
        )
        .expect("duplicate stderr text");

        let err = managed_helper_exit_result_with_replay(
            ExitStatus::from_raw(1 << 8),
            130,
            temp.path(),
            Some(&log),
            false,
        )
        .expect_err("extra worker stderr text must not be classified as known duplicate noise");

        assert!(format!("{err:#}").contains("managed loftd helper exited with status 1"));
    }

    #[test]
    fn managed_helper_status_rejects_cleanup_failure_despite_marker() {
        let temp = tempfile::tempdir().expect("tempdir");
        managed_exit_marker::write_observed_guest_exit(temp.path(), 130).expect("marker");
        let log = ManagedHelperStderr::create(temp.path()).expect("stderr log");
        std::fs::write(
            &log.path,
            "failed after managed guest exited with status 130: cleanup failure\n",
        )
        .expect("stderr text");

        let err = managed_helper_exit_result_with_replay(
            ExitStatus::from_raw(1 << 8),
            130,
            temp.path(),
            Some(&log),
            false,
        )
        .expect_err("cleanup failure must take precedence");

        assert!(format!("{err:#}").contains("managed loftd helper exited with status 1"));
    }

    #[test]
    fn managed_helper_status_rejects_nonzero_without_matching_marker() {
        let temp = tempfile::tempdir().expect("tempdir");
        let log = ManagedHelperStderr::create(temp.path()).expect("stderr log");

        let err = managed_helper_exit_result_with_replay(
            ExitStatus::from_raw(1 << 8),
            130,
            temp.path(),
            Some(&log),
            false,
        )
        .expect_err("missing marker must not be suppressed");

        assert!(format!("{err:#}").contains("managed loftd helper exited with status 1"));
    }
}

fn context_audit_error(
    err: anyhow::Error,
    audit_trace_path: Option<&Path>,
    context: &'static str,
) -> anyhow::Error {
    if audit_trace_path.is_some() {
        err.context(format!("{context}; {}", seccomp::ptrace_failure_hint()))
    } else {
        err.context(context)
    }
}

fn terminate_spawned_child_group(child: &mut Child) {
    let _ = kill_spawned_process_group(child.id(), libc::SIGTERM);
    let deadline = Instant::now() + Duration::from_millis(500);
    while Instant::now() < deadline {
        match child.try_wait() {
            Ok(Some(_)) => return,
            Ok(None) => thread::sleep(Duration::from_millis(25)),
            Err(_) => break,
        }
    }
    let _ = kill_spawned_process_group(child.id(), libc::SIGKILL);
    let _ = child.kill();
    let _ = child.wait();
}

fn start_sigwinch_forwarder(pgid: u32) -> Result<Option<SigwinchForwarder>> {
    if !stdin_is_tty() {
        return Ok(None);
    }
    SigwinchForwarder::start(pgid).map(Some)
}

fn stdin_is_tty() -> bool {
    // Only interactive tty-bound helpers need SIGWINCH rebroadcasting.
    unsafe { libc::isatty(libc::STDIN_FILENO) == 1 }
}

fn kill_spawned_process_group(pgid: u32, signal: i32) -> Result<()> {
    let pgid = i32::try_from(pgid).context("spawned process group id does not fit in i32")?;
    if pgid <= 1 {
        bail!("refusing to signal unsafe spawned process group id {pgid}");
    }
    let rc = unsafe { libc::kill(-pgid, signal) };
    if rc == 0 {
        return Ok(());
    }
    Err(std::io::Error::last_os_error())
        .with_context(|| format!("failed to signal spawned helper process group {pgid}"))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HelperCommandSpec {
    pub(crate) program: OsString,
    pub(crate) env: Vec<(OsString, OsString)>,
    pub(crate) args: Vec<OsString>,
}

impl HelperCommandSpec {
    fn into_command(self) -> Command {
        let mut command = Command::new(self.program);
        command.args(self.args);
        command.envs(self.env);
        command
    }
}

fn build_helper_command(
    executable: &Path,
    config_path: &Path,
    config: &LaunchConfig,
    host_profile_enabled: bool,
    managed_ready_fd: Option<i32>,
) -> Result<HelperCommandSpec> {
    let launcher = KeepIdLauncher::from_current_system()?;
    tracing::debug!(summary = %launcher.diagnostic_summary(), "loftd libkrun helper keep-id namespace resolved");
    Ok(build_helper_command_with_launcher(
        executable,
        config_path,
        config.log_level,
        host_profile_enabled,
        managed_ready_fd,
        &launcher,
    ))
}

pub(crate) fn build_helper_command_with_launcher(
    executable: &Path,
    config_path: &Path,
    log_level: crate::logging::LogLevel,
    host_profile_enabled: bool,
    managed_ready_fd: Option<i32>,
    launcher: &KeepIdLauncher,
) -> HelperCommandSpec {
    HelperCommandSpec {
        program: launcher.program(),
        env: helper_env(log_level, host_profile_enabled, managed_ready_fd),
        args: launcher.args(executable, LIBKRUN_ENTER_HELPER_ARG, config_path),
    }
}

fn helper_env(
    log_level: crate::logging::LogLevel,
    host_profile_enabled: bool,
    managed_ready_fd: Option<i32>,
) -> Vec<(OsString, OsString)> {
    let mut env = vec![(
        OsString::from(INTERNAL_LOG_LEVEL_ENV),
        OsString::from(log_level.as_str()),
    )];
    if host_profile_enabled {
        env.push((OsString::from(LOFTD_HOST_PROFILE_ENV), OsString::from("1")));
    }
    if let Some(fd) = managed_ready_fd {
        env.push((OsString::from(READY_FD_ENV), OsString::from(fd.to_string())));
    }
    env
}
