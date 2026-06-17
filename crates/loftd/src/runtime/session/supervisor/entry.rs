//! Internal helper entrypoint and launch-config dispatch.

use anyhow::{Context, Result, anyhow, bail};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::logging::{self, LogSettings};
use crate::runtime::launch::config::{LaunchConfig, NetworkMode};
use crate::runtime::session::profile::LoftdHostProfiler;
use crate::runtime::session::rootfs::task::{UnsharedBtrfsRootfsCommands, cleanup_task_rootfs_dir};
use crate::runtime::session::supervisor::LIBKRUN_ENTER_HELPER_ARG;
use crate::runtime::session::supervisor::identity;
use crate::runtime::session::supervisor::rlimits;
use crate::runtime::session::supervisor::vm_child::{self, VmWorkerGuard};
use crate::runtime::session::task_control;
use crate::runtime::vm::network::{NetworkManagerSession, PasstWorkerSession, status_exit_code};

pub(crate) fn run_internal(args: Vec<OsString>) -> Result<()> {
    let [subcommand, config_path]: [OsString; 2] = args.try_into().map_err(|args: Vec<_>| {
        anyhow!(
            "expected internal {LIBKRUN_ENTER_HELPER_ARG} <launch.conf>, got {} argument(s)",
            args.len()
        )
    })?;
    if subcommand.to_str() != Some(LIBKRUN_ENTER_HELPER_ARG) {
        anyhow::bail!(
            "unknown loftd internal command '{}'; expected {LIBKRUN_ENTER_HELPER_ARG}",
            subcommand.to_string_lossy()
        );
    }
    run_helper(PathBuf::from(config_path).as_path())
}

fn run_helper(config_path: &Path) -> Result<()> {
    let mut profiler = LoftdHostProfiler::from_env_started_now();
    profiler.record_metadata("profile_scope", "helper");
    profiler.record_metadata(
        "profile_launch_config_path",
        config_path.display().to_string(),
    );

    let result = run_helper_profiled(config_path, &mut profiler);
    profiler.finalize_result(result)
}

fn run_helper_profiled(config_path: &Path, profiler: &mut LoftdHostProfiler) -> Result<()> {
    if logging::helper_pre_config_debug_enabled() {
        eprintln!(
            "loftd internal: libkrun-network-enter starting config={}",
            config_path.display()
        );
    }
    let config = profiler.measure_result("helper_config_read", || {
        LaunchConfig::read_from(config_path)
    })?;
    profiler.measure_result("helper_tracing_init", || {
        logging::init_tracing(&LogSettings::for_internal_helper(config.log_level))
    })?;
    profiler.measure_result("helper_identity_configure", || {
        identity::configure_helper_filesystem_identity_for_launch(&config)
    })?;
    profiler.measure_result("helper_nofile_rlimit_raise", || {
        rlimits::raise_host_nofile_soft_limit()
    })?;
    let task_state_dir = task_state_dir_from_config_path(config_path)?;
    profiler.record_metadata(
        "profile_task_state_dir",
        task_state_dir.display().to_string(),
    );
    tracing::debug!(
        mode = config.network_mode.as_config_value(),
        "loftd internal: network manager starting"
    );
    let network_session = profiler.measure_result("helper_network_start", || {
        NetworkManagerSession::start(task_state_dir, config.network_mode, &config.publish)
    })?;
    let passt_session = if config.network_mode == NetworkMode::Passt {
        Some(profiler.measure_result("helper_passt_start", || {
            PasstWorkerSession::start(&config.publish)
        })?)
    } else {
        None
    };
    let passt_fd = passt_session.as_ref().map(PasstWorkerSession::fd);
    let mut worker =
        VmWorkerGuard::new(profiler.measure_result("helper_vm_worker_fork", || {
            vm_child::fork_vm_worker(config_path, network_session.holder_pid(), passt_fd)
        })?);
    let (status, wait_duration) =
        profiler.measure_result_with_duration("helper_wait_vm_worker", || worker.wait())?;
    profiler.record_vm_worker_wait_details(task_state_dir, wait_duration);
    let cleanup_error = cleanup_managed_task_after_vm_exit(&config, task_state_dir).err();
    if let Some(code) = status_exit_code(status) {
        if code == 0 {
            if let Some(err) = cleanup_error {
                return Err(err);
            }
            return Ok(());
        }
        if let Some(err) = cleanup_error {
            return Err(err).context(format!("loftd VM worker exited with status {code}"));
        }
        bail!("loftd VM worker exited with status {code}");
    }
    if let Some(err) = cleanup_error {
        return Err(err).context("loftd VM worker exited due to signal");
    }
    bail!("loftd VM worker exited due to signal")
}

fn cleanup_managed_task_after_vm_exit(config: &LaunchConfig, task_state_dir: &Path) -> Result<()> {
    let Some(managed) = &config.managed_session else {
        return Ok(());
    };
    task_control::remove_active_task(task_state_dir)?;
    let _ = std::fs::remove_file(&managed.attach_socket);
    if managed.cleanup_task_rootfs_on_exit {
        cleanup_task_rootfs_dir(task_state_dir, &UnsharedBtrfsRootfsCommands)?;
    }
    Ok(())
}

pub(crate) fn task_state_dir_from_config_path(config_path: &Path) -> Result<&Path> {
    config_path.parent().ok_or_else(|| {
        anyhow!(
            "loftd launch config '{}' must live inside a task state directory",
            config_path.display()
        )
    })
}
