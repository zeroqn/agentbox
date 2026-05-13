use anyhow::Result;

use crate::mounts::format::format_mount_arg_with_options;
use crate::runtime::container::nix_sidecar::SidecarNixRuntime;
use crate::{
    CONTAINER_NIX_DIR, CONTAINER_SCCACHE_DIR, CONTAINER_TMP_TMPFS, CONTAINER_WORKDIR,
    INTERACTIVE_SHELL, NIX_REMOTE_SOCKET, TASK_CONTAINER_ROLE_LABEL, TASK_CONTAINER_ROLE_VALUE,
    TASK_CONTAINER_SIDECAR_LABEL,
};

pub(crate) struct ContainerTaskPodmanSpec<'a> {
    pub(crate) image: &'a str,
    pub(crate) container_name: &'a str,
    pub(crate) hostname: &'a str,
    pub(crate) workspace_mount: &'a str,
    pub(crate) codex_mount: &'a str,
    pub(crate) cargo_mount: &'a str,
    pub(crate) sccache_mount: &'a str,
    pub(crate) nix_runtime: &'a SidecarNixRuntime,
    pub(crate) guest_profile: bool,
    pub(crate) guest_debug: bool,
}

pub(crate) const GUEST_PROFILE_ENV: &str = "AGENTBOX_GUEST_PROFILE=1";
pub(crate) const GUEST_DEBUG_ENV: &str = "AGENTBOX_GUEST_DEBUG=1";

pub(crate) fn build_container_task_podman_args(
    spec: ContainerTaskPodmanSpec<'_>,
) -> Result<Vec<String>> {
    let sidecar = spec.nix_runtime;
    let mut args = vec![
        "run".to_owned(),
        "--rm".to_owned(),
        "-it".to_owned(),
        "--name".to_owned(),
        spec.container_name.to_owned(),
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
        "--volume".to_owned(),
        format_mount_arg_with_options(&sidecar.merged_dir, CONTAINER_NIX_DIR, Some("ro"))?,
        "--env".to_owned(),
        format!("NIX_REMOTE={NIX_REMOTE_SOCKET}"),
        "--label".to_owned(),
        format!("{TASK_CONTAINER_ROLE_LABEL}={TASK_CONTAINER_ROLE_VALUE}"),
        "--label".to_owned(),
        format!("{TASK_CONTAINER_SIDECAR_LABEL}={}", sidecar.sidecar_name),
    ];

    if spec.guest_profile {
        args.push("--env".to_owned());
        args.push(GUEST_PROFILE_ENV.to_owned());
    }

    if spec.guest_debug {
        args.push("--env".to_owned());
        args.push(GUEST_DEBUG_ENV.to_owned());
    }

    args.push(spec.image.to_owned());
    args.push(INTERACTIVE_SHELL.to_owned());
    args.push("-l".to_owned());

    Ok(args)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn container_task_args_include_sidecar_nix_mount_and_remote() {
        let runtime = sidecar_runtime();
        let args = build_args(&runtime);

        assert_eq!(args[0], "run");
        assert!(args.contains(&"--name".to_owned()));
        assert!(args.contains(&"project-random".to_owned()));
        assert_eq!(args[5], "--userns");
        assert_eq!(args[6], "keep-id");
        assert!(args.contains(&"/tmp/project:/workspace".to_owned()));
        assert!(args.contains(&"/home/alice/.codex:/home/dev/.codex".to_owned()));
        assert!(args.contains(&"/tmp/state/agentbox/project/cargo:/home/dev/.cargo".to_owned()));
        assert!(args.contains(&"/tmp/state/agentbox/sccache:/home/dev/.cache/sccache".to_owned()));
        assert!(args.contains(&"/tmp/state/agentbox/project/nix-merged:/nix:ro".to_owned()));
        assert!(args.contains(&format!("SCCACHE_DIR={CONTAINER_SCCACHE_DIR}")));
        assert!(args.contains(&format!("NIX_REMOTE={NIX_REMOTE_SOCKET}")));
        assert!(args.contains(&format!(
            "{TASK_CONTAINER_ROLE_LABEL}={TASK_CONTAINER_ROLE_VALUE}"
        )));
        assert!(args.contains(&format!(
            "{TASK_CONTAINER_SIDECAR_LABEL}=agentbox-nix-sidecar-abc"
        )));
        assert!(args.contains(&CONTAINER_TMP_TMPFS.to_owned()));
        assert_eq!(args[args.len() - 2], INTERACTIVE_SHELL);
        assert_eq!(args[args.len() - 1], "-l");
    }

    #[test]
    fn container_task_args_exclude_seeded_and_libkrun_runtime_args() {
        let runtime = sidecar_runtime();
        let args = build_args(&runtime);
        let joined = args.join("\n");

        assert!(!joined.contains("/nix/store:/nix/store"));
        assert!(!joined.contains("/nix/var/nix:/nix/var/nix"));
        assert!(!joined.contains("AGENTBOX_KVM_DROP_TO_DEV"));
        assert!(!joined.contains("AGENTBOX_HOST_UID"));
        assert!(!joined.contains("AGENTBOX_HOST_GID"));
        assert!(!joined.contains("NIX_PROXY"));
        assert!(!joined.contains("krun."));
        assert!(!joined.contains("run.oci.handler=krun"));
        assert!(!joined.contains("no_proxy=1"));
        assert!(!args.contains(&"--runtime".to_owned()));
    }

    fn sidecar_runtime() -> SidecarNixRuntime {
        SidecarNixRuntime {
            merged_dir: PathBuf::from("/tmp/state/agentbox/project/nix-merged"),
            sidecar_name: "agentbox-nix-sidecar-abc".to_owned(),
            proxy_port: 19876,
            mount_mode: crate::runtime::container::nix_sidecar::PodmanImageMountMode::Direct,
        }
    }

    fn build_args(nix_runtime: &SidecarNixRuntime) -> Vec<String> {
        build_container_task_podman_args(ContainerTaskPodmanSpec {
            image: crate::DEFAULT_IMAGE,
            container_name: "project-random",
            hostname: "project-agentbox",
            workspace_mount: "/tmp/project:/workspace",
            codex_mount: "/home/alice/.codex:/home/dev/.codex",
            cargo_mount: "/tmp/state/agentbox/project/cargo:/home/dev/.cargo",
            sccache_mount: "/tmp/state/agentbox/sccache:/home/dev/.cache/sccache",
            nix_runtime,
            guest_profile: false,
            guest_debug: false,
        })
        .expect("container task args should build")
    }

    #[test]
    fn container_task_args_include_guest_profile_and_debug_env_when_requested() {
        let runtime = sidecar_runtime();
        let args = build_container_task_podman_args(ContainerTaskPodmanSpec {
            image: crate::DEFAULT_IMAGE,
            container_name: "project-random",
            hostname: "project-agentbox",
            workspace_mount: "/tmp/project:/workspace",
            codex_mount: "/home/alice/.codex:/home/dev/.codex",
            cargo_mount: "/tmp/state/agentbox/project/cargo:/home/dev/.cargo",
            sccache_mount: "/tmp/state/agentbox/sccache:/home/dev/.cache/sccache",
            nix_runtime: &runtime,
            guest_profile: true,
            guest_debug: true,
        })
        .expect("container task args should build");

        assert!(args.contains(&GUEST_PROFILE_ENV.to_owned()));
        assert!(args.contains(&GUEST_DEBUG_ENV.to_owned()));
        assert_eq!(args[args.len() - 2], INTERACTIVE_SHELL);
        assert_eq!(args[args.len() - 1], "-l");
    }
}
