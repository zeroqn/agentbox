//! Host-side libkrun helper command construction and process spawning.

use anyhow::{Context, Result, bail};
use std::ffi::OsString;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use crate::logging::INTERNAL_LOG_LEVEL_ENV;
use crate::runtime::launch::config::LaunchConfig;
use crate::runtime::seccomp;
use crate::runtime::session::attach::{self, AttachOutcome};
use crate::runtime::session::profile::{LOFTD_HOST_PROFILE_ENV, LoftdHostProfiler};
use crate::runtime::session::supervisor::identity::KeepIdLauncher;
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
    if config.managed_session.is_some() {
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit());
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
            return Err(context_audit_error(
                err,
                audit_trace_path.as_deref(),
                "failed while waiting for managed loftd attach readiness",
            ));
        }
        let attach_result = profiler.measure_result("helper_initial_attach", || {
            attach::attach_to_ready_socket(&managed.attach_socket, daemon_initial_attach)
        });
        return match attach_result {
            Ok(AttachOutcome::Detached) => Ok(ChildStatus::detached()),
            Ok(AttachOutcome::Exited(code)) => {
                let _ = child.wait();
                Ok(ChildStatus::exited(code))
            }
            Err(err) => {
                terminate_spawned_child_group(&mut child);
                let _ = remove_active_task(&active_task.task_dir);
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
