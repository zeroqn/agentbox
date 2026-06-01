use anyhow::{Context, Result, bail};
use std::env;
use std::ffi::OsString;
use std::process::ExitCode;

use crate::cli::Cli;
use crate::task_rootfs::TaskRootfsBackend;

pub(crate) mod ffi;
pub(crate) mod guest_init;
pub(crate) mod image_source;
pub(crate) mod launch_config;
pub(crate) mod launch_plan;
pub(crate) mod persistent_disks;
pub(crate) mod raw_btrfs;
pub(crate) mod supervisor;
pub(crate) mod task_rootfs;

use launch_plan::LaunchPlan;
use persistent_disks::{HostPersistentDiskPreparer, PersistentDiskPreparer};
use supervisor::{HostSupervisor, Supervisor};
use task_rootfs::{HostBtrfsRootfsCommands, TaskRootfsLease, TaskRootfsManager};

pub(crate) fn run(cli: Cli) -> Result<ExitCode> {
    let cwd = env::current_dir()?;
    let plan = LaunchPlan::from_env(cli.into_runtime_options(), cwd)?;

    if plan.debug {
        eprintln!(
            "loftd: launch plan: image={} rootfs-backend={} state-root={} image-cache={} sccache={} workspace-slug={} config={} loaded={}",
            plan.image_selection.selected_reference(),
            plan.task_rootfs_backend,
            plan.state_layout.root_dir().display(),
            plan.image_cache_dir.display(),
            plan.sccache_dir.display(),
            plan.workspace_slug,
            plan.config_diagnostics.config_path.display(),
            plan.config_diagnostics.config_loaded,
        );
    }

    match plan.task_rootfs_backend {
        TaskRootfsBackend::BtrfsSnapshot => {
            let task_id = task_rootfs::new_task_id(&plan.workspace_slug);
            let manager = TaskRootfsManager::new(plan.state_layout.root_dir().to_path_buf());
            let handle = manager.materialize_btrfs_from_buildah(
                &plan.image_selection,
                &task_id,
                plan.preserve_debug,
                &image_source::HostBuildahCommands,
                &HostBtrfsRootfsCommands,
            )?;
            let lease = TaskRootfsLease::new(handle, HostBtrfsRootfsCommands);

            if plan.debug {
                let handle = lease.handle();
                let digest = handle.image_digest().unwrap_or("<unknown>");
                eprintln!(
                    "loftd: task rootfs: task-id={} backend={} image={} digest={} rootfs={} task-dir={}",
                    handle.task_id(),
                    handle.backend(),
                    handle.selected_image_reference(),
                    digest,
                    handle.rootfs_path().display(),
                    handle.task_dir().display(),
                );
            }

            let disks = HostPersistentDiskPreparer
                .prepare(plan.state_layout.root_dir())
                .context("failed to prepare loftd persistent dev cache disks")?;
            let guest_init = guest_init::resolve_guest_init(
                lease.handle().rootfs_path(),
                plan.guest_init.as_deref(),
            )?;
            let config = launch_config::LaunchConfig::build_for_task(launch_config::LaunchSpec {
                task_rootfs: lease.handle().rootfs_path(),
                workspace_source: &plan.workspace_dir,
                guest_init_exec: &guest_init.guest_exec_path,
                guest_command: &plan.guest_command,
                mem_gib: plan.mem_gib,
                debug: plan.debug,
                profile: plan.profile,
                root: plan.root,
                host_uid: current_uid(),
                host_gid: current_gid(),
                vcpus: launch_config::resolve_cpu_count()?,
                disks: disks.attachments(),
                extra_env: disks.env_pairs(),
            })?;

            if plan.debug {
                eprintln!(
                    "loftd: libkrun launch: guest-init={} disks={} ram-mib={} vcpus={} workspace={}",
                    guest_init.guest_exec_path,
                    config.disks.len(),
                    config.ram_mib,
                    config.vcpus,
                    plan.workspace_dir.display(),
                );
            }

            let run_result = HostSupervisor.run(&config, lease.handle().task_dir());
            if plan.preserve_debug {
                let hint = lease.handle().preserve_debug_hint();
                let result = lease.preserve();
                if let task_rootfs::CleanupResult::Preserved(path) = result {
                    eprintln!(
                        "loftd: preserving task rootfs '{}' because --preserve-debug was set; {hint}",
                        path.display()
                    );
                }
                return run_result.map(|status| status.exit_code());
            }
            let status = match run_result {
                Ok(status) => status,
                Err(run_error) => {
                    lease.cleanup().with_context(|| {
                        format!(
                            "failed to clean loftd task rootfs after libkrun helper error: {run_error:#}"
                        )
                    })?;
                    return Err(run_error);
                }
            };
            lease.cleanup()?;
            Ok(status.exit_code())
        }
        TaskRootfsBackend::FuseOverlay => bail!(
            "loftd fuse-overlay task rootfs materialization is not implemented in this phase; use btrfs-snapshot or wait for the fuse-overlay slice"
        ),
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
