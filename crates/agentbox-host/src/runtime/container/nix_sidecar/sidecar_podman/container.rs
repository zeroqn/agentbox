use anyhow::Result;
use std::path::Path;
use std::process::Stdio;

use crate::podman::command::run_podman;
use crate::podman::volume::format_mount_arg;
use crate::runtime::container::nix_sidecar::sidecar_podman::proxy::SIDECAR_PROXY_CONTAINER_PORT;
use crate::CONTAINER_NIX_DIR;

pub(in crate::runtime::container::nix_sidecar) const SIDECAR_ENTRYPOINT: &str =
    "/bin/agentbox-nix-sidecar-entrypoint";

pub(in crate::runtime::container::nix_sidecar) fn cleanup_sidecar_container(
    sidecar_name: &str,
) -> Result<()> {
    let args = vec!["rm".to_owned(), "-f".to_owned(), sidecar_name.to_owned()];
    let _ = run_podman(
        args,
        Stdio::null(),
        Stdio::null(),
        Stdio::null(),
        "failed to remove stale sidecar container",
    );
    Ok(())
}

pub(in crate::runtime::container::nix_sidecar) fn build_sidecar_podman_args(
    image: &str,
    sidecar_name: &str,
    merged_mount: &str,
) -> Result<Vec<String>> {
    let mut args = vec![
        "run".to_owned(),
        "-d".to_owned(),
        "--name".to_owned(),
        sidecar_name.to_owned(),
        "--user".to_owned(),
        "0:0".to_owned(),
        "--volume".to_owned(),
        merged_mount.to_owned(),
        "--publish".to_owned(),
        SIDECAR_PROXY_CONTAINER_PORT.to_owned(),
    ];

    args.extend([
        "--entrypoint".to_owned(),
        SIDECAR_ENTRYPOINT.to_owned(),
        image.to_owned(),
    ]);

    Ok(args)
}

pub(in crate::runtime::container::nix_sidecar) fn build_merged_mount_arg(
    merged_dir: &Path,
) -> Result<String> {
    format_mount_arg(merged_dir, CONTAINER_NIX_DIR)
}
