use anyhow::{Context, Result, bail};
use std::env;
use std::ffi::OsString;
use std::process::ExitCode;

use crate::cli::RuntimeOptions;
use crate::task_rootfs::TaskRootfsBackend;

pub(crate) mod ffi;
pub(crate) mod guest_init;
pub(crate) mod image_source;
pub(crate) mod launch_config;
pub(crate) mod launch_plan;
pub(crate) mod persistent_disks;
pub(crate) mod prepared_root;
mod profile;
pub(crate) mod raw_btrfs;
pub(crate) mod supervisor;
pub(crate) mod task_rootfs;

use launch_plan::LaunchPlan;
use persistent_disks::{HostPersistentDiskPreparer, PersistentDiskPreparer};
use profile::LoftdHostProfiler;
use supervisor::{HostSupervisor, Supervisor};
use task_rootfs::{HostBtrfsRootfsCommands, TaskRootfsLease, TaskRootfsManager};

pub(crate) fn run(options: RuntimeOptions) -> Result<ExitCode> {
    let cwd = env::current_dir()?
        .canonicalize()
        .context("failed to canonicalize current directory for loftd workspace mount")?;
    let mut profiler = LoftdHostProfiler::new(host_profile_enabled(&options));
    let plan =
        profiler.measure_result("launch_plan_build", || LaunchPlan::from_env(options, cwd))?;
    profiler.record_metadata(
        "task_rootfs_backend",
        plan.task_rootfs_backend.as_config_value(),
    );
    profiler.record_metadata("image", plan.image_selection.selected_reference());

    tracing::debug!(
        image = plan.image_selection.selected_reference(),
        rootfs_backend = %plan.task_rootfs_backend,
        state_root = %plan.state_layout.root_dir().display(),
        image_cache = %plan.image_cache_dir.display(),
        sccache = %plan.sccache_dir.display(),
        mounts = plan.bind_mounts.len(),
        workspace_slug = %plan.workspace_slug,
        config = %plan.config_diagnostics.config_path.display(),
        loaded = plan.config_diagnostics.config_loaded,
        "loftd launch plan"
    );

    match plan.task_rootfs_backend {
        TaskRootfsBackend::BtrfsSnapshot => {
            let task_id = task_rootfs::new_task_id(&plan.workspace_slug);
            let manager = TaskRootfsManager::new(plan.state_layout.root_dir().to_path_buf());
            let handle = profiler.measure_result("task_rootfs_materialization", || {
                manager.materialize_btrfs_from_buildah(
                    &plan.image_selection,
                    &task_id,
                    plan.preserve_debug,
                    &image_source::HostBuildahCommands,
                    &HostBtrfsRootfsCommands,
                )
            })?;
            let lease = TaskRootfsLease::new(handle, HostBtrfsRootfsCommands);
            if let Some(digest) = lease.handle().image_digest() {
                profiler.record_metadata("image_digest", digest);
            }

            {
                let handle = lease.handle();
                let digest = handle.image_digest().unwrap_or("<unknown>");
                tracing::debug!(
                    task_id = handle.task_id(),
                    backend = %handle.backend(),
                    image = handle.selected_image_reference(),
                    digest,
                    rootfs = %handle.rootfs_path().display(),
                    task_dir = %handle.task_dir().display(),
                    "loftd task rootfs"
                );
            }

            let host_run_result = match profiler
                .measure_result("persistent_disk_preparation", || {
                    HostPersistentDiskPreparer.prepare(plan.state_layout.root_dir())
                })
                .context("failed to prepare loftd persistent dev cache disks")
            {
                Ok(disks) => match profiler.measure_result("guest_init_resolution", || {
                    guest_init::resolve_guest_init_with_entrypoint(
                        lease.handle().rootfs_path(),
                        plan.guest_init.as_deref(),
                        lease.handle().process_config().entrypoint.as_slice(),
                    )
                }) {
                    Ok(guest_init) => match profiler.measure_result("launch_config_build", || {
                        launch_config::LaunchConfig::build_for_task(launch_config::LaunchSpec {
                            task_rootfs: lease.handle().rootfs_path(),
                            mounts: &plan.bind_mounts,
                            guest_init_exec: &guest_init.guest_exec_path,
                            guest_command: &plan.guest_command,
                            image_process_config: lease.handle().process_config(),
                            mem_gib: plan.mem_gib,
                            log_level: plan.log_level,
                            profile: plan.profile,
                            root: plan.root,
                            host_uid: current_uid(),
                            host_gid: current_gid(),
                            vcpus: launch_config::resolve_cpu_count()?,
                            disks: disks.attachments(),
                            extra_env: disks.env_pairs(),
                        })
                    }) {
                        Ok(config) => {
                            tracing::debug!(
                                guest_init = %guest_init.guest_exec_path,
                                disks = config.disks.len(),
                                ram_mib = config.ram_mib,
                                vcpus = config.vcpus,
                                workspace = %plan.workspace_dir.display(),
                                mounts = config.mounts.len(),
                                "loftd libkrun launch"
                            );

                            BtrfsHostRunResult::Helper(
                                profiler.measure_result("helper_session", || {
                                    HostSupervisor.run(&config, lease.handle().task_dir())
                                }),
                            )
                        }
                        Err(err) => BtrfsHostRunResult::SetupFailed(err),
                    },
                    Err(err) => BtrfsHostRunResult::SetupFailed(err),
                },
                Err(err) => BtrfsHostRunResult::SetupFailed(err),
            };
            let cleanup_result = if plan.preserve_debug {
                let hint = lease.handle().preserve_debug_hint();
                profiler.measure_result("task_state_cleanup", || {
                    let result = lease.preserve();
                    if let task_rootfs::CleanupResult::Preserved(path) = &result {
                        eprintln!(
                            "loftd: preserving task rootfs '{}' because --preserve-debug was set; {hint}",
                            path.display()
                        );
                    }
                    Ok(result)
                })
            } else {
                profiler.measure_result("task_state_cleanup", || lease.cleanup())
            };
            profiler.emit_to_stderr();
            match host_run_result {
                BtrfsHostRunResult::Helper(run_result) => {
                    finalize_btrfs_run_result(run_result, cleanup_result)
                }
                BtrfsHostRunResult::SetupFailed(setup_error) => {
                    finalize_post_lease_setup_failure(setup_error, cleanup_result)
                }
            }
        }
        TaskRootfsBackend::FuseOverlay => bail!(
            "loftd fuse-overlay task rootfs materialization is not implemented in this phase; use btrfs-snapshot or wait for the fuse-overlay slice"
        ),
    }
}

enum BtrfsHostRunResult {
    Helper(Result<supervisor::ChildStatus>),
    SetupFailed(anyhow::Error),
}

fn host_profile_enabled(options: &RuntimeOptions) -> bool {
    options.profile && options.log_settings.level.enables_debug()
}

fn finalize_post_lease_setup_failure(
    setup_error: anyhow::Error,
    cleanup_result: Result<task_rootfs::CleanupResult>,
) -> Result<ExitCode> {
    if let Err(cleanup_error) = cleanup_result {
        eprintln!(
            "loftd: best-effort task rootfs cleanup failed after post-materialization error: {cleanup_error:#}"
        );
    }
    Err(setup_error)
}

fn finalize_btrfs_run_result(
    run_result: Result<supervisor::ChildStatus>,
    cleanup_result: Result<task_rootfs::CleanupResult>,
) -> Result<ExitCode> {
    match (run_result, cleanup_result) {
        (Ok(status), Ok(_)) => Ok(status.exit_code()),
        (Ok(_), Err(cleanup_error)) => Err(cleanup_error),
        (Err(run_error), Ok(_)) => Err(run_error),
        (Err(run_error), Err(cleanup_error)) => Err(cleanup_error.context(format!(
            "failed to clean loftd task rootfs after libkrun helper error: {run_error:#}"
        ))),
    }
}

pub(crate) fn run_internal(args: Vec<OsString>) -> Result<()> {
    if args.first().and_then(|arg| arg.to_str()) == Some(supervisor::LIBKRUN_ENTER_HELPER_ARG) {
        return supervisor::run_internal(args);
    }
    image_source::run_internal(args)
}

fn current_uid() -> u32 {
    unsafe { libc::getuid() }
}

fn current_gid() -> u32 {
    unsafe { libc::getgid() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::anyhow;
    use std::path::PathBuf;

    fn runtime_options(debug: bool, profile: bool) -> RuntimeOptions {
        RuntimeOptions {
            image: None,
            pull_latest: false,
            debug,
            log_settings: crate::logging::LogSettings::resolve(None, debug, None),
            profile,
            root: false,
            rootfs_backend: None,
            guest_init: None,
            preserve_debug: false,
            mem_gib: None,
            guest_command: Vec::new(),
        }
    }

    #[test]
    fn host_profile_is_enabled_only_when_debug_and_profile_are_enabled() {
        assert!(!host_profile_enabled(&runtime_options(false, false)));
        assert!(!host_profile_enabled(&runtime_options(true, false)));
        assert!(!host_profile_enabled(&runtime_options(false, true)));
        assert!(host_profile_enabled(&runtime_options(true, true)));
    }

    #[test]
    fn btrfs_finalization_returns_helper_exit_code_after_cleanup_success() {
        let code = finalize_btrfs_run_result(
            Ok(supervisor::ChildStatus::exited(42)),
            Ok(task_rootfs::CleanupResult::Removed),
        )
        .expect("successful helper and cleanup should return helper exit code");

        assert_eq!(code, ExitCode::from(42));
    }

    #[test]
    fn btrfs_finalization_returns_helper_exit_code_after_preserve_success() {
        let code = finalize_btrfs_run_result(
            Ok(supervisor::ChildStatus::exited(7)),
            Ok(task_rootfs::CleanupResult::Preserved(PathBuf::from(
                "/tmp/loftd-task/rootfs",
            ))),
        )
        .expect("successful helper and preserve should return helper exit code");

        assert_eq!(code, ExitCode::from(7));
    }

    #[test]
    fn post_lease_setup_failure_is_preserved_after_cleanup_success() {
        let err = finalize_post_lease_setup_failure(
            anyhow!("persistent disk failure"),
            Ok(task_rootfs::CleanupResult::Removed),
        )
        .expect_err("setup failure should be preserved when cleanup succeeds");

        assert_eq!(err.to_string(), "persistent disk failure");
    }

    #[test]
    fn post_lease_setup_failure_is_preserved_after_cleanup_failure() {
        let err = finalize_post_lease_setup_failure(
            anyhow!("guest-init failure"),
            Err(anyhow!("cleanup failure")),
        )
        .expect_err("setup failure should still be preserved when best-effort cleanup fails");

        assert_eq!(err.to_string(), "guest-init failure");
    }

    #[test]
    fn btrfs_finalization_cleanup_error_dominates_successful_helper() {
        let err = finalize_btrfs_run_result(
            Ok(supervisor::ChildStatus::exited(0)),
            Err(anyhow!("cleanup failure")),
        )
        .expect_err("cleanup failure should dominate successful helper");

        assert_eq!(err.to_string(), "cleanup failure");
    }

    #[test]
    fn btrfs_finalization_preserves_helper_error_after_cleanup_success() {
        let err = finalize_btrfs_run_result(
            Err(anyhow!("helper failure")),
            Ok(task_rootfs::CleanupResult::Removed),
        )
        .expect_err("helper failure should be preserved when cleanup succeeds");

        assert_eq!(err.to_string(), "helper failure");
    }

    #[test]
    fn btrfs_finalization_cleanup_error_after_helper_error_is_contextualized() {
        let err = finalize_btrfs_run_result(
            Err(anyhow!("helper failure")),
            Err(anyhow!("cleanup failure")),
        )
        .expect_err("cleanup failure should dominate helper failure with context");
        let text = format!("{err:#}");

        assert!(text.contains("cleanup failure"));
        assert!(text.contains("failed to clean loftd task rootfs after libkrun helper error"));
        assert!(text.contains("helper failure"));
    }
}
