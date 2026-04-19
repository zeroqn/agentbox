use anyhow::Result;

use crate::mounts::format::{format_mount_arg, format_mount_arg_with_options};
use crate::{
    NixRuntime, CONTAINER_NIX_DIR, CONTAINER_TMP_TMPFS, CONTAINER_WORKDIR, INTERACTIVE_SHELL,
    NIX_REMOTE_SOCKET, TASK_CONTAINER_ROLE_LABEL, TASK_CONTAINER_ROLE_VALUE,
    TASK_CONTAINER_SIDECAR_LABEL,
};

pub fn build_podman_args(
    image: &str,
    hostname: &str,
    workspace_mount: &str,
    codex_mount: &str,
    cargo_mount: &str,
    nix_runtime: &NixRuntime,
) -> Result<Vec<String>> {
    let mut args = vec![
        "run".to_owned(),
        "--rm".to_owned(),
        "-it".to_owned(),
        "--userns".to_owned(),
        "keep-id".to_owned(),
        "--workdir".to_owned(),
        CONTAINER_WORKDIR.to_owned(),
        "--hostname".to_owned(),
        hostname.to_owned(),
        "--volume".to_owned(),
        workspace_mount.to_owned(),
        "--volume".to_owned(),
        codex_mount.to_owned(),
        "--volume".to_owned(),
        cargo_mount.to_owned(),
        "--tmpfs".to_owned(),
        CONTAINER_TMP_TMPFS.to_owned(),
    ];

    match nix_runtime {
        NixRuntime::Seeded(persistent_nix_root) => {
            for (source, destination) in persistent_nix_root.mounts() {
                args.push("--volume".to_owned());
                args.push(format_mount_arg(source, destination)?);
            }
        }
        NixRuntime::Sidecar(sidecar) => {
            args.push("--volume".to_owned());
            args.push(format_mount_arg_with_options(
                &sidecar.merged_dir,
                CONTAINER_NIX_DIR,
                Some("ro"),
            )?);
            args.push("--env".to_owned());
            args.push(format!("NIX_REMOTE={NIX_REMOTE_SOCKET}"));
            args.push("--label".to_owned());
            args.push(format!(
                "{TASK_CONTAINER_ROLE_LABEL}={TASK_CONTAINER_ROLE_VALUE}"
            ));
            args.push("--label".to_owned());
            args.push(format!(
                "{TASK_CONTAINER_SIDECAR_LABEL}={}",
                sidecar.sidecar_name
            ));
        }
    }

    args.push(image.to_owned());
    args.push(INTERACTIVE_SHELL.to_owned());
    args.push("-l".to_owned());
    Ok(args)
}
