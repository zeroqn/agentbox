use anyhow::Result;

use crate::mounts::format::{format_mount_arg, format_mount_arg_with_options};
use crate::{
    NixRuntime, TaskContainerMode, CONTAINER_NIX_DIR, CONTAINER_SCCACHE_DIR, CONTAINER_TMP_TMPFS,
    CONTAINER_WORKDIR, INTERACTIVE_SHELL, NIX_REMOTE_SOCKET, TASK_CONTAINER_ROLE_LABEL,
    TASK_CONTAINER_ROLE_VALUE, TASK_CONTAINER_SIDECAR_LABEL,
};

pub struct TaskPodmanSpec<'a> {
    pub image: &'a str,
    pub hostname: &'a str,
    pub workspace_mount: &'a str,
    pub codex_mount: &'a str,
    pub cargo_mount: &'a str,
    pub sccache_mount: &'a str,
    pub nix_runtime: &'a NixRuntime,
    pub task_mode: TaskContainerMode,
}

pub fn build_podman_args(spec: TaskPodmanSpec<'_>) -> Result<Vec<String>> {
    let mut args = vec![
        "run".to_owned(),
        "--rm".to_owned(),
        "-it".to_owned(),
        "--userns".to_owned(),
        "keep-id".to_owned(),
        "--workdir".to_owned(),
        CONTAINER_WORKDIR.to_owned(),
        "--hostname".to_owned(),
        spec.hostname.to_owned(),
        "--volume".to_owned(),
        spec.workspace_mount.to_owned(),
        "--volume".to_owned(),
        spec.codex_mount.to_owned(),
        "--volume".to_owned(),
        spec.cargo_mount.to_owned(),
        "--volume".to_owned(),
        spec.sccache_mount.to_owned(),
        "--env".to_owned(),
        format!("SCCACHE_DIR={CONTAINER_SCCACHE_DIR}"),
        "--tmpfs".to_owned(),
        CONTAINER_TMP_TMPFS.to_owned(),
    ];

    if spec.task_mode == TaskContainerMode::KvmKrunExperimental {
        args.push("--runtime".to_owned());
        args.push("crun".to_owned());
        args.push("--annotation".to_owned());
        args.push("run.oci.handler=krun".to_owned());
    }

    match spec.nix_runtime {
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

    args.push(spec.image.to_owned());
    args.push(INTERACTIVE_SHELL.to_owned());
    args.push("-l".to_owned());
    Ok(args)
}
