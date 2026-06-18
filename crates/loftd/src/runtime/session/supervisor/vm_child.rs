//! Forked VM child process and direct libkrun entry path.

use anyhow::{Result, bail};
use std::path::Path;
use std::time::Instant;

use crate::runtime::launch::config::{LaunchConfig, NetworkMode};
use crate::runtime::session::nix_overlay;
use crate::runtime::session::profile::{LoftdHostProfiler, vm_worker_wait_detail_path};
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
}

impl Drop for VmWorkerGuard {
    fn drop(&mut self) {
        if self.pid > 0 {
            network::cleanup_pid(self.pid);
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
}
