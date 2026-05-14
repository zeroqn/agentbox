use anyhow::Result;

use crate::mounts::format::format_mount_arg_with_options;
use crate::podman::run::{RunArgOwner, RunSpec};
use crate::{
    CONTAINER_NIX_DIR, NIX_REMOTE_SOCKET, TASK_CONTAINER_ROLE_LABEL, TASK_CONTAINER_ROLE_VALUE,
    TASK_CONTAINER_SIDECAR_LABEL,
};

use super::SidecarNixRuntime;

pub(crate) const SIDECAR_NIX_OWNER: RunArgOwner = RunArgOwner::new("runtime.container.nix_sidecar");

pub(crate) fn append_task_args(run: &mut RunSpec, sidecar: &SidecarNixRuntime) -> Result<()> {
    run.option(
        SIDECAR_NIX_OWNER,
        "--volume",
        format_mount_arg_with_options(&sidecar.merged_dir, CONTAINER_NIX_DIR, Some("ro"))?,
    );
    run.option(
        SIDECAR_NIX_OWNER,
        "--env",
        format!("NIX_REMOTE={NIX_REMOTE_SOCKET}"),
    );
    run.option(
        SIDECAR_NIX_OWNER,
        "--label",
        format!("{TASK_CONTAINER_ROLE_LABEL}={TASK_CONTAINER_ROLE_VALUE}"),
    );
    run.option(
        SIDECAR_NIX_OWNER,
        "--label",
        format!("{TASK_CONTAINER_SIDECAR_LABEL}={}", sidecar.sidecar_name),
    );

    Ok(())
}
