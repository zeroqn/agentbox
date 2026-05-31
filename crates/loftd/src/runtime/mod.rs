use anyhow::{Result, bail};
use std::env;
use std::process::ExitCode;

use crate::cli::Cli;

pub(crate) mod launch_plan;

use launch_plan::LaunchPlan;

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

    bail!(
        "loftd microvm runtime launch is not implemented yet (state root: {}, image: {}, task rootfs backend: {})",
        plan.state_layout.root_dir().display(),
        plan.image_selection.selected_reference(),
        plan.task_rootfs_backend,
    );
}
