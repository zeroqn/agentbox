//! Forked VM child process and direct libkrun entry path.

use anyhow::{Context, Result, anyhow, bail};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use crate::logging::{self, LogSettings};
use crate::runtime::host_tools::{RuntimeTool, runtime_tool_program};
use crate::runtime::launch::config::{LaunchConfig, NetworkMode};
use crate::runtime::seccomp::{self, AuditMode, SeccompMode};
use crate::runtime::session::nix_overlay;
use crate::runtime::session::profile::{LoftdHostProfiler, vm_worker_wait_detail_path};
use crate::runtime::session::supervisor::LIBKRUN_VM_WORKER_ARG;
use crate::runtime::session::supervisor::entry::task_state_dir_from_config_path;
use crate::runtime::session::supervisor::identity;
use crate::runtime::session::supervisor::readiness_pipe::HelperReadyWriter;
use crate::runtime::vm::libkrun::{DirectLibkrunLauncher, DynamicLibkrunApi};
use crate::runtime::vm::network;
use crate::runtime::vm::prepared_root;

pub(crate) struct VmWorkerGuard {
    pid: libc::pid_t,
}

impl VmWorkerGuard {
    pub(crate) fn new(pid: libc::pid_t) -> Self {
        Self { pid }
    }

    pub(crate) fn wait(&mut self) -> Result<i32> {
        let status = network::wait_pid(self.pid)?;
        self.pid = -1;
        Ok(status)
    }

    pub(crate) fn try_wait(&mut self) -> Result<Option<i32>> {
        if self.pid <= 0 {
            return Ok(None);
        }
        let mut status = 0;
        // SAFETY: pid is a child process id returned by fork.
        let rc = unsafe { libc::waitpid(self.pid, &mut status, libc::WNOHANG) };
        if rc == 0 {
            return Ok(None);
        }
        if rc == self.pid {
            self.pid = -1;
            return Ok(Some(status));
        }
        bail!(
            "failed to poll loftd VM worker {}: {}",
            self.pid,
            std::io::Error::last_os_error()
        );
    }

    pub(crate) fn terminate(&mut self) {
        if self.pid > 0 {
            network::cleanup_pid(self.pid);
            self.pid = -1;
        }
    }
}

impl Drop for VmWorkerGuard {
    fn drop(&mut self) {
        self.terminate();
    }
}

pub(crate) fn start_vm_worker(
    config_path: &Path,
    holder_pid: libc::pid_t,
    passt_fd: Option<i32>,
    seccomp_mode: &SeccompMode,
) -> Result<VmWorkerGuard> {
    match seccomp_mode {
        SeccompMode::Audit(audit_mode) => {
            spawn_traced_vm_worker(config_path, holder_pid, passt_fd, audit_mode)
        }
        SeccompMode::Off | SeccompMode::Enforce { .. } => {
            fork_vm_worker(config_path, holder_pid, passt_fd).map(VmWorkerGuard::new)
        }
    }
}

pub(crate) fn fork_vm_worker(
    config_path: &Path,
    holder_pid: libc::pid_t,
    passt_fd: Option<i32>,
) -> Result<libc::pid_t> {
    // SAFETY: fork creates an isolated worker process that enters the target netns and exits.
    let pid = unsafe { libc::fork() };
    if pid < 0 {
        bail!(
            "failed to fork loftd VM worker: {}",
            std::io::Error::last_os_error()
        );
    }
    if pid == 0 {
        HelperReadyWriter::close_in_vm_worker_child_from_env();
        std::process::exit(run_vm_worker_child(config_path, holder_pid, passt_fd));
    }
    Ok(pid)
}

fn spawn_traced_vm_worker(
    config_path: &Path,
    holder_pid: libc::pid_t,
    passt_fd: Option<i32>,
    audit_mode: &AuditMode,
) -> Result<VmWorkerGuard> {
    let trace_path = audit_mode.trace_path();
    seccomp::prepare_audit_trace_target(trace_path)?;
    let executable = std::env::current_exe()
        .context("failed to resolve loftd executable for traced VM worker")?;
    let strace_filter = match audit_mode {
        AuditMode::Full { .. } => None,
        AuditMode::Gap {
            baseline_policy_path,
            ..
        } => Some(seccomp::strace_exclusion_filter_from_policy(
            baseline_policy_path,
        )?),
        AuditMode::DefaultGap { .. } => {
            bail!("unresolved default seccomp gap audit cannot reach the VM worker")
        }
    };
    let spec = build_traced_vm_worker_command(
        &executable,
        trace_path,
        strace_filter.as_deref(),
        config_path,
        holder_pid,
        passt_fd,
    );
    tracing::debug!(program = ?spec.program, args = ?spec.args, "loftd traced VM worker command constructed");
    let child = spec.into_command().spawn().with_context(|| {
        format!(
            "failed to start traced loftd VM worker for '{}'; {}",
            config_path.display(),
            seccomp::ptrace_failure_hint()
        )
    })?;
    let pid = libc::pid_t::try_from(child.id()).context("traced VM worker pid overflowed pid_t")?;
    Ok(VmWorkerGuard::new(pid))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct VmWorkerCommandSpec {
    pub(crate) program: OsString,
    pub(crate) args: Vec<OsString>,
}

impl VmWorkerCommandSpec {
    fn into_command(self) -> Command {
        let mut command = Command::new(self.program);
        command.args(self.args);
        command
    }
}

pub(crate) fn build_traced_vm_worker_command(
    executable: &Path,
    trace_path: &Path,
    strace_filter: Option<&str>,
    config_path: &Path,
    holder_pid: libc::pid_t,
    passt_fd: Option<i32>,
) -> VmWorkerCommandSpec {
    let raw_path = seccomp::raw_strace_path(trace_path);
    let mut args = vec![OsString::from("-f"), OsString::from("-qq")];
    if let Some(filter) = strace_filter {
        args.push(OsString::from("-e"));
        args.push(OsString::from(filter));
    }
    args.extend([
        OsString::from("-o"),
        raw_path.into_os_string(),
        OsString::from("--"),
        executable.as_os_str().to_owned(),
        OsString::from("internal"),
        OsString::from(LIBKRUN_VM_WORKER_ARG),
        config_path.as_os_str().to_owned(),
        OsString::from(holder_pid.to_string()),
    ]);
    if let Some(fd) = passt_fd {
        args.push(OsString::from(fd.to_string()));
    }
    VmWorkerCommandSpec {
        program: runtime_tool_program(RuntimeTool::Strace),
        args,
    }
}

pub(crate) fn run_vm_worker_internal(args: Vec<OsString>) -> Result<()> {
    let invocation = parse_vm_worker_internal_args(args)?;
    HelperReadyWriter::close_in_vm_worker_child_from_env();
    std::process::exit(run_vm_worker_child(
        &invocation.config_path,
        invocation.holder_pid,
        invocation.passt_fd,
    ));
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct VmWorkerInternalInvocation {
    pub(crate) config_path: PathBuf,
    pub(crate) holder_pid: libc::pid_t,
    pub(crate) passt_fd: Option<i32>,
}

pub(crate) fn parse_vm_worker_internal_args(
    args: Vec<OsString>,
) -> Result<VmWorkerInternalInvocation> {
    if !(2..=3).contains(&args.len()) {
        bail!(
            "expected internal {LIBKRUN_VM_WORKER_ARG} <launch.conf> <holder-pid> [passt-fd], got {} argument(s)",
            args.len() + 1
        );
    }
    let mut args = args.into_iter();
    let config_path = args
        .next()
        .expect("validated VM worker internal config argument");
    let holder_pid = args
        .next()
        .expect("validated VM worker internal holder pid argument");
    let holder_pid = parse_i32_arg("holder-pid", holder_pid)?;
    let passt_fd = args
        .next()
        .map(|fd| parse_i32_arg("passt-fd", fd))
        .transpose()?;
    Ok(VmWorkerInternalInvocation {
        config_path: PathBuf::from(config_path),
        holder_pid,
        passt_fd,
    })
}

fn parse_i32_arg(label: &str, value: OsString) -> Result<i32> {
    value
        .to_str()
        .ok_or_else(|| anyhow!("internal {label} argument is not valid UTF-8"))?
        .parse::<i32>()
        .with_context(|| format!("internal {label} argument is not a valid i32"))
}

fn run_vm_worker_child(config_path: &Path, holder_pid: libc::pid_t, passt_fd: Option<i32>) -> i32 {
    let mut profiler = LoftdHostProfiler::from_env_started_now();
    profiler.record_metadata("profile_scope", "vm_worker");
    profiler.record_metadata(
        "profile_launch_config_path",
        config_path.display().to_string(),
    );

    let result = run_vm_worker(config_path, holder_pid, passt_fd, &mut profiler);
    let exit_code = if result.is_ok() { 0 } else { 1 };
    profiler.record_metadata("profile_exit_code", exit_code.to_string());
    if let Err(err) = &result {
        eprintln!("loftd internal VM worker: {err:#}");
    }
    if let Ok(task_state_dir) = task_state_dir_from_config_path(config_path) {
        let _ = profiler.write_vm_worker_wait_details(task_state_dir);
    }
    let _ = profiler.finalize_result(result);
    exit_code
}

fn run_vm_worker(
    config_path: &Path,
    holder_pid: libc::pid_t,
    passt_fd: Option<i32>,
    profiler: &mut LoftdHostProfiler,
) -> Result<()> {
    let config = profiler.measure_result("vm_worker_config_read", || {
        LaunchConfig::read_from(config_path)
    })?;
    profiler.measure_result("vm_worker_tracing_init", || {
        logging::init_tracing(&LogSettings::for_internal_helper(config.log_level))
    })?;
    profiler.measure_result("vm_worker_identity_configure", || {
        identity::configure_vm_worker_filesystem_identity()
    })?;
    profiler.measure_result("vm_worker_enter_netns", || network::enter_netns(holder_pid))?;
    let task_state_dir = task_state_dir_from_config_path(config_path)?;
    profiler.record_metadata(
        "profile_task_state_dir",
        task_state_dir.display().to_string(),
    );
    let config = match config.network_mode {
        NetworkMode::Tsi => config,
        NetworkMode::Passt => profiler.measure_result("vm_worker_passt_fd_handoff", || {
            passt_fd
                .map(|fd| config.with_passt_fd(fd))
                .ok_or_else(|| anyhow::anyhow!("loftd passt mode requires inherited passt fd"))
        })?,
    };
    run_libkrun_in_current_namespace(&config, task_state_dir, profiler)
}

fn run_libkrun_in_current_namespace(
    config: &LaunchConfig,
    task_state_dir: &Path,
    profiler: &mut LoftdHostProfiler,
) -> Result<()> {
    let nix_overlay_mount = match config.host_nix_overlay.as_ref() {
        Some(intent) => Some(profiler.measure_result("vm_worker_mount_nix_overlay", || {
            nix_overlay::materialize_in_worker(intent)
        })?),
        None => None,
    };
    let prepared_root = profiler.measure_result("vm_worker_prepare_root", || {
        prepared_root::prepare(config, task_state_dir)
    })?;
    let run_result =
        run_libkrun_with_prepared_root(config, task_state_dir, profiler, &prepared_root);
    let nix_overlay_cleanup = nix_overlay_mount.map(|mount| move || mount.unmount());
    finalize_vm_worker_run(run_result, || prepared_root.unmount(), nix_overlay_cleanup)
}

fn run_libkrun_with_prepared_root(
    config: &LaunchConfig,
    task_state_dir: &Path,
    profiler: &mut LoftdHostProfiler,
    prepared_root: &prepared_root::PreparedRoot,
) -> Result<()> {
    let launch_config = config.with_root_export(prepared_root.root().to_path_buf());
    let guest_config_path = profiler.measure_result("vm_worker_guest_config_write", || {
        launch_config.write_guest_config_to_rootfs()
    })?;
    tracing::debug!(
        task_state = %task_state_dir.display(),
        source_rootfs = %config.task_rootfs.display(),
        rootfs = %launch_config.task_rootfs.display(),
        guest_config = %guest_config_path.display(),
        prepared_root_bind_count = launch_config.mounts.len(),
        disks = launch_config.disks.len(),
        ram_mib = launch_config.ram_mib,
        vcpus = launch_config.vcpus,
        exec_path = %launch_config.exec_path,
        argv_len = launch_config.argv.len(),
        env_len = launch_config.env.len(),
        guest_config_env_len = launch_config.guest_config_env.len(),
        "loftd internal: launch config loaded"
    );
    tracing::debug!("libkrun API open: begin");
    let api = profiler.measure_result("vm_worker_libkrun_open", || {
        DynamicLibkrunApi::open_default()
    })?;
    tracing::debug!("libkrun API open: complete");
    let session_started_at = Instant::now();
    let configure_started_at = Instant::now();
    let mut configure_duration = std::time::Duration::ZERO;
    let mut pre_enter_reached = false;
    let libkrun_profile_path = if profiler.is_enabled() {
        Some(vm_worker_wait_detail_path(task_state_dir))
    } else {
        None
    };
    let result = DirectLibkrunLauncher::new(api).start_enter_profiled_with_pre_enter_hook(
        &launch_config,
        libkrun_profile_path.as_deref(),
        || {
            configure_duration = configure_started_at.elapsed();
            pre_enter_reached = true;
            profiler.record_vm_worker_libkrun_configure(configure_duration);
            let _ = profiler.write_vm_worker_wait_details(task_state_dir);
            if let SeccompMode::Enforce { policy_path } = &launch_config.seccomp {
                seccomp::apply_enforce_policy(policy_path)?;
            }
            tracing::debug!(
                seccomp = launch_config.seccomp.as_config_value(),
                "loftd internal: pre-enter hook complete"
            );
            Ok(())
        },
    );
    let session_duration = session_started_at.elapsed();
    profiler.record_vm_worker_libkrun_session(session_duration);
    if pre_enter_reached {
        profiler
            .record_vm_worker_libkrun_enter(session_duration.saturating_sub(configure_duration));
    }
    result
}

fn finalize_vm_worker_run<PreparedCleanup, NixCleanup>(
    run_result: Result<()>,
    cleanup_prepared_root: PreparedCleanup,
    cleanup_nix_overlay: Option<NixCleanup>,
) -> Result<()>
where
    PreparedCleanup: FnOnce() -> Result<()>,
    NixCleanup: FnOnce() -> Result<()>,
{
    let prepared_root_cleanup = cleanup_prepared_root();
    let nix_overlay_cleanup = cleanup_nix_overlay.map(|cleanup| cleanup());
    let cleanup_result = combine_cleanup_results(prepared_root_cleanup, nix_overlay_cleanup);
    combine_run_and_cleanup_results(run_result, cleanup_result)
}

fn combine_cleanup_results(
    prepared_root_cleanup: Result<()>,
    nix_overlay_cleanup: Option<Result<()>>,
) -> Result<()> {
    match (prepared_root_cleanup, nix_overlay_cleanup) {
        (Ok(()), Some(Ok(()))) | (Ok(()), None) => Ok(()),
        (Ok(()), Some(Err(cleanup_error))) => Err(cleanup_error),
        (Err(cleanup_error), Some(Ok(()))) | (Err(cleanup_error), None) => Err(cleanup_error),
        (Err(prepared_error), Some(Err(nix_error))) => Err(prepared_error.context(format!(
            "also failed to unmount loftd host /nix overlay: {nix_error:#}"
        ))),
    }
}

fn combine_run_and_cleanup_results(
    run_result: Result<()>,
    cleanup_result: Result<()>,
) -> Result<()> {
    match (run_result, cleanup_result) {
        (Ok(()), Ok(())) => Ok(()),
        (Ok(()), Err(cleanup_error)) => Err(cleanup_error),
        (Err(run_error), Ok(())) => Err(run_error),
        (Err(run_error), Err(cleanup_error)) => Err(cleanup_error.context(format!(
            "failed to clean up loftd VM worker mounts after libkrun error: {run_error:#}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::anyhow;
    use std::cell::RefCell;
    use std::ffi::OsStr;

    #[test]
    fn finalization_unmounts_prepared_root_before_nix_overlay() {
        let calls = RefCell::new(Vec::new());

        finalize_vm_worker_run(
            Ok(()),
            || {
                calls.borrow_mut().push("prepared-root");
                Ok(())
            },
            Some(|| {
                calls.borrow_mut().push("nix-overlay");
                Ok(())
            }),
        )
        .expect("finalization should succeed");

        assert_eq!(calls.into_inner(), vec!["prepared-root", "nix-overlay"]);
    }

    #[test]
    fn finalization_attempts_nix_overlay_after_prepared_root_cleanup_failure() {
        let calls = RefCell::new(Vec::new());

        let err = finalize_vm_worker_run(
            Ok(()),
            || {
                calls.borrow_mut().push("prepared-root");
                Err(anyhow!("prepared-root busy"))
            },
            Some(|| {
                calls.borrow_mut().push("nix-overlay");
                Ok(())
            }),
        )
        .expect_err("prepared-root failure should surface");

        assert_eq!(calls.into_inner(), vec!["prepared-root", "nix-overlay"]);
        assert!(format!("{err:#}").contains("prepared-root busy"));
    }

    #[test]
    fn finalization_contextualizes_cleanup_failure_after_run_failure() {
        let err = finalize_vm_worker_run(
            Err(anyhow!("libkrun failed")),
            || Err(anyhow!("prepared-root busy")),
            None::<fn() -> Result<()>>,
        )
        .expect_err("cleanup failure should surface with run context");

        let message = format!("{err:#}");
        assert!(message.contains("prepared-root busy"));
        assert!(message.contains("libkrun failed"));
    }

    #[test]
    fn traced_vm_worker_command_targets_private_worker_entrypoint() {
        let spec = build_traced_vm_worker_command(
            Path::new("/nix/store/bin/loftd"),
            Path::new("/tmp/loftd-seccomp.trace.jsonl"),
            None,
            Path::new("/tmp/task/launch.conf"),
            12345,
            None,
        );

        assert_eq!(
            spec.args,
            vec![
                OsString::from("-f"),
                OsString::from("-qq"),
                OsString::from("-o"),
                OsString::from("/tmp/loftd-seccomp.trace.jsonl.strace"),
                OsString::from("--"),
                OsString::from("/nix/store/bin/loftd"),
                OsString::from("internal"),
                OsString::from(LIBKRUN_VM_WORKER_ARG),
                OsString::from("/tmp/task/launch.conf"),
                OsString::from("12345"),
            ]
        );
    }

    #[test]
    fn traced_vm_worker_command_includes_gap_audit_filter() {
        let spec = build_traced_vm_worker_command(
            Path::new("/nix/store/bin/loftd"),
            Path::new("/tmp/loftd-seccomp.denied.jsonl"),
            Some("trace=!read,write"),
            Path::new("/tmp/task/launch.conf"),
            12345,
            None,
        );

        assert_eq!(
            spec.args,
            vec![
                OsString::from("-f"),
                OsString::from("-qq"),
                OsString::from("-e"),
                OsString::from("trace=!read,write"),
                OsString::from("-o"),
                OsString::from("/tmp/loftd-seccomp.denied.jsonl.strace"),
                OsString::from("--"),
                OsString::from("/nix/store/bin/loftd"),
                OsString::from("internal"),
                OsString::from(LIBKRUN_VM_WORKER_ARG),
                OsString::from("/tmp/task/launch.conf"),
                OsString::from("12345"),
            ]
        );
    }

    #[test]
    fn traced_vm_worker_command_preserves_passt_fd_argument() {
        let spec = build_traced_vm_worker_command(
            Path::new("/bin/loftd"),
            Path::new("/tmp/trace.jsonl"),
            None,
            Path::new("/tmp/task/launch.conf"),
            99,
            Some(42),
        );

        assert_eq!(
            spec.args.last().map(OsString::as_os_str),
            Some(OsStr::new("42"))
        );
    }

    #[test]
    fn vm_worker_internal_args_parse_required_and_optional_values() {
        let invocation = parse_vm_worker_internal_args(vec![
            "/tmp/task/launch.conf".into(),
            "123".into(),
            "44".into(),
        ])
        .expect("VM worker internal args should parse");

        assert_eq!(
            invocation.config_path,
            PathBuf::from("/tmp/task/launch.conf")
        );
        assert_eq!(invocation.holder_pid, 123);
        assert_eq!(invocation.passt_fd, Some(44));

        let invocation =
            parse_vm_worker_internal_args(vec!["/tmp/task/launch.conf".into(), "123".into()])
                .expect("VM worker internal args should parse without passt fd");
        assert_eq!(invocation.passt_fd, None);
    }

    #[test]
    fn vm_worker_internal_args_reject_bad_shape_and_numbers() {
        let err = parse_vm_worker_internal_args(vec!["/tmp/task/launch.conf".into()])
            .expect_err("missing holder pid should fail");
        assert!(format!("{err:#}").contains("expected internal"));

        let err =
            parse_vm_worker_internal_args(vec!["/tmp/task/launch.conf".into(), "not-a-pid".into()])
                .expect_err("invalid holder pid should fail");
        assert!(format!("{err:#}").contains("holder-pid"));
    }
}
