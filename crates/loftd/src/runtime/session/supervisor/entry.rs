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
use crate::runtime::session::supervisor::managed_ready;
use crate::runtime::session::supervisor::readiness_pipe::HelperReadyWriter;
use crate::runtime::session::supervisor::rlimits;
use crate::runtime::session::supervisor::vm_child::{self, VmWorkerGuard};
use crate::runtime::session::task_control;
use crate::runtime::vm::network::{NetworkManagerSession, PasstWorkerSession, status_exit_code};
use crate::runtime::vm::prepared_root;

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
    let mut ready_writer = HelperReadyWriter::from_env()?;
    let result = run_helper_profiled_inner(config_path, profiler, &mut ready_writer);
    if let Err(err) = &result
        && let Some(writer) = ready_writer.take()
    {
        let _ = writer.send_error(&format!("{err:#}"));
    }
    result
}

fn run_helper_profiled_inner(
    config_path: &Path,
    profiler: &mut LoftdHostProfiler,
    ready_writer: &mut Option<HelperReadyWriter>,
) -> Result<()> {
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
    if let Some(managed) = &config.managed_session
        && let Some(writer) = ready_writer.take()
    {
        match profiler.measure_result("helper_managed_attach_ready", || {
            managed_ready::wait_for_managed_attach_socket(managed, &mut worker)
        }) {
            Ok(()) => writer.send_ready()?,
            Err(err) => {
                let _ = writer.send_error(&format!("{err:#}"));
                return Err(err);
            }
        }
    }
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
    cleanup_managed_task_after_vm_exit_with(
        config,
        task_state_dir,
        || prepared_root::cleanup_existing(config, task_state_dir),
        || cleanup_task_rootfs_dir(task_state_dir, &UnsharedBtrfsRootfsCommands),
    )
}

fn cleanup_managed_task_after_vm_exit_with<PreparedRootCleanup, TaskRootfsCleanup>(
    config: &LaunchConfig,
    task_state_dir: &Path,
    cleanup_prepared_root: PreparedRootCleanup,
    cleanup_task_rootfs: TaskRootfsCleanup,
) -> Result<()>
where
    PreparedRootCleanup: FnOnce() -> Result<()>,
    TaskRootfsCleanup: FnOnce() -> Result<()>,
{
    let Some(managed) = &config.managed_session else {
        return Ok(());
    };
    task_control::remove_active_task(task_state_dir)?;
    let _ = std::fs::remove_file(&managed.attach_socket);
    if managed.cleanup_task_rootfs_on_exit {
        cleanup_prepared_root()
            .context("failed to clean up loftd prepared-root before managed task rootfs removal")?;
        cleanup_task_rootfs()?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logging::LogLevel;
    use crate::runtime::launch::config::ManagedSessionConfig;
    use std::cell::RefCell;

    #[test]
    fn managed_cleanup_runs_prepared_root_fallback_before_task_dir_removal() {
        let temp = tempfile::tempdir().expect("tempdir");
        let task_dir = temp.path().join("task");
        std::fs::create_dir_all(&task_dir).expect("task dir");
        let config = managed_config(&task_dir, true);
        let calls = RefCell::new(Vec::new());

        cleanup_managed_task_after_vm_exit_with(
            &config,
            &task_dir,
            || {
                calls.borrow_mut().push("prepared-root");
                Ok(())
            },
            || {
                calls.borrow_mut().push("task-rootfs");
                Ok(())
            },
        )
        .expect("managed cleanup should succeed");

        assert_eq!(calls.into_inner(), vec!["prepared-root", "task-rootfs"]);
    }

    #[test]
    fn managed_cleanup_skips_rootfs_cleanup_when_preserving_task_state() {
        let temp = tempfile::tempdir().expect("tempdir");
        let task_dir = temp.path().join("task");
        std::fs::create_dir_all(&task_dir).expect("task dir");
        let config = managed_config(&task_dir, false);
        let calls = RefCell::new(Vec::new());

        cleanup_managed_task_after_vm_exit_with(
            &config,
            &task_dir,
            || {
                calls.borrow_mut().push("prepared-root");
                Ok(())
            },
            || {
                calls.borrow_mut().push("task-rootfs");
                Ok(())
            },
        )
        .expect("managed cleanup should succeed");

        assert!(calls.into_inner().is_empty());
    }

    fn managed_config(task_dir: &Path, cleanup_task_rootfs_on_exit: bool) -> LaunchConfig {
        LaunchConfig {
            task_rootfs: task_dir.join("rootfs"),
            hostname: "loftd-test".to_owned(),
            mounts: Vec::new(),
            host_nix_overlay: None,
            guest_init_override: None,
            disks: Vec::new(),
            ram_mib: 1024,
            vcpus: 1,
            log_level: LogLevel::Info,
            network_mode: NetworkMode::Tsi,
            publish: Vec::new(),
            workdir: "/workspace".to_owned(),
            exec_path: "/bin/sh".to_owned(),
            argv: Vec::new(),
            env: Vec::new(),
            guest_config_env: Vec::new(),
            passt_fd: None,
            managed_session: Some(ManagedSessionConfig {
                attach_socket: task_dir.join("attach.sock"),
                guest_port: 1025,
                protocol_version: 1,
                attach_socket_uid: 1000,
                attach_socket_gid: 1000,
                cleanup_task_rootfs_on_exit,
            }),
            seccomp: Default::default(),
        }
    }
}
