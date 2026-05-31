use anyhow::{Result, bail};
use std::env;
use std::ffi::OsString;
use std::process::ExitCode;

use crate::cli::Cli;
use crate::task_rootfs::TaskRootfsBackend;

pub(crate) mod image_source;
pub(crate) mod launch_plan;
pub(crate) mod task_rootfs;

use launch_plan::LaunchPlan;
use task_rootfs::{HostBtrfsRootfsCommands, TaskRootfsManager};

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

            if plan.debug {
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

            let not_implemented = format!(
                "loftd libkrun boot is not implemented yet (task rootfs: {}, image: {}, backend: {})",
                handle.rootfs_path().display(),
                handle.selected_image_reference(),
                handle.backend(),
            );
            if plan.preserve_debug {
                bail!("{}; {}", not_implemented, handle.preserve_debug_hint());
            }
            handle.cleanup_state(&HostBtrfsRootfsCommands)?;
            bail!(
                "{}; cleaned task rootfs because the boot slice is not implemented",
                not_implemented
            );
        }
        TaskRootfsBackend::FuseOverlay => bail!(
            "loftd fuse-overlay task rootfs materialization is not implemented in this phase; use btrfs-snapshot or wait for the fuse-overlay slice"
        ),
    }
}

pub(crate) fn run_internal(args: Vec<OsString>) -> Result<()> {
    image_source::run_internal(args)
}
