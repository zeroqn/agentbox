//! Forked VM child process and direct libkrun entry path.

use anyhow::{Result, bail};
use std::os::fd::OwnedFd;
use std::path::Path;
use std::time::Instant;

use crate::runtime::launch::config::{LaunchConfig, NetworkMode};
use crate::runtime::session::profile::{LoftdHostProfiler, vm_worker_wait_detail_path};
use crate::runtime::session::supervisor::entry::task_state_dir_from_config_path;
use crate::runtime::session::supervisor::identity;
use crate::runtime::vm::libkrun::{DirectLibkrunLauncher, DynamicLibkrunApi};
use crate::runtime::vm::network::{self, PasstWorkerSession};
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
    passt_pid_pipe: Option<OwnedFd>,
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
        std::process::exit(run_vm_worker_child(config_path, holder_pid, passt_pid_pipe));
    }
    Ok(pid)
}

fn run_vm_worker_child(
    config_path: &Path,
    holder_pid: libc::pid_t,
    passt_pid_pipe: Option<OwnedFd>,
) -> i32 {
    let mut profiler = LoftdHostProfiler::from_env_started_now();
    profiler.record_metadata("profile_scope", "vm_worker");
    profiler.record_metadata(
        "profile_launch_config_path",
        config_path.display().to_string(),
    );

    let result = run_vm_worker(config_path, holder_pid, passt_pid_pipe, &mut profiler);
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
    passt_pid_pipe: Option<OwnedFd>,
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
    let (config, _passt_session) = match config.network_mode {
        NetworkMode::Tsi => (config, None),
        NetworkMode::Passt => profiler.measure_result("vm_worker_passt_start", || {
            let session = PasstWorkerSession::start(task_state_dir, &config.publish)?;
            if let Some(pipe) = passt_pid_pipe {
                network::write_passt_pid(pipe, session.pid())?;
            }
            Ok((
                config.with_passt_socket(session.socket().to_path_buf()),
                Some(session),
            ))
        })?,
    };
    run_libkrun_in_current_namespace(&config, task_state_dir, profiler)
}

fn run_libkrun_in_current_namespace(
    config: &LaunchConfig,
    task_state_dir: &Path,
    profiler: &mut LoftdHostProfiler,
) -> Result<()> {
    let prepared_root = profiler.measure_result("vm_worker_prepare_root", || {
        prepared_root::prepare(config, task_state_dir)
    })?;
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
