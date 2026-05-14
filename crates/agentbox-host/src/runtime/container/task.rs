use anyhow::Result;

use crate::podman::run::{RunArgs, RunSpec, CORE};
use crate::runtime::components::{diagnostics, identity, volumes};
use crate::runtime::container::nix_sidecar::{append_task_sidecar_nix_args, SidecarNixRuntime};
use crate::{CONTAINER_TMP_TMPFS, CONTAINER_WORKDIR, INTERACTIVE_SHELL};

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

pub(crate) fn build_container_task_podman_args(
    spec: ContainerTaskPodmanSpec<'_>,
) -> Result<Vec<String>> {
    Ok(build_container_task_run_args(spec)?.into_vec())
}

fn build_container_task_run_args(spec: ContainerTaskPodmanSpec<'_>) -> Result<RunArgs> {
    let mut run = RunSpec::new();

    run.args(CORE, ["run", "--rm", "-it"]);
    run.option(CORE, "--name", spec.container_name);
    identity::append_userns_keep_id(&mut run);
    run.option(CORE, "--workdir", CONTAINER_WORKDIR);
    run.option(CORE, "--hostname", spec.hostname);
    volumes::append_workspace(&mut run, spec.workspace_mount);
    volumes::append_codex(&mut run, spec.codex_mount);
    volumes::append_cargo(&mut run, spec.cargo_mount);
    volumes::append_sccache(&mut run, spec.sccache_mount);
    run.option(CORE, "--tmpfs", CONTAINER_TMP_TMPFS);
    append_task_sidecar_nix_args(&mut run, spec.nix_runtime)?;
    diagnostics::append_guest_diagnostics(&mut run, spec.guest_profile, spec.guest_debug);
    run.arg(CORE, spec.image);
    run.args(CORE, [INTERACTIVE_SHELL, "-l"]);

    Ok(run.render())
}

#[cfg(test)]
mod tests {
    use crate::runtime::components::diagnostics::{
        GUEST_DEBUG_ENV, GUEST_DIAGNOSTICS_OWNER, GUEST_PROFILE_ENV,
    };
    use crate::runtime::components::identity::USER_IDENTITY_OWNER;
    use crate::runtime::components::volumes::{
        CARGO_VOLUME_OWNER, CODEX_VOLUME_OWNER, SCCACHE_VOLUME_OWNER, WORKSPACE_VOLUME_OWNER,
    };
    use crate::runtime::container::nix_sidecar::SIDECAR_NIX_OWNER;
    use crate::runtime::container::nix_sidecar::{PodmanImageMountMode, SidecarNixRuntime};
    use crate::runtime::container::task::{
        build_container_task_podman_args, build_container_task_run_args, ContainerTaskPodmanSpec,
    };
    use crate::{
        CONTAINER_SCCACHE_DIR, CONTAINER_TMP_TMPFS, INTERACTIVE_SHELL, NIX_REMOTE_SOCKET,
        TASK_CONTAINER_ROLE_LABEL, TASK_CONTAINER_ROLE_VALUE, TASK_CONTAINER_SIDECAR_LABEL,
    };
    use std::path::PathBuf;

    #[test]
    fn container_task_args_match_ordered_default_baseline() {
        let runtime = sidecar_runtime();
        let args = build_args(&runtime);

        assert_eq!(args, default_expected_args());
    }

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

    #[test]
    fn container_task_args_expose_component_owner_ownership() {
        let runtime = sidecar_runtime();
        let args = build_run_args(&runtime);

        assert!(args.contains_option_from(USER_IDENTITY_OWNER, "--userns", "keep-id"));
        assert!(args.contains_option_from(
            WORKSPACE_VOLUME_OWNER,
            "--volume",
            "/tmp/project:/workspace"
        ));
        assert!(args.contains_option_from(
            CODEX_VOLUME_OWNER,
            "--volume",
            "/home/alice/.codex:/home/dev/.codex"
        ));
        assert!(args.contains_option_from(
            CARGO_VOLUME_OWNER,
            "--volume",
            "/tmp/state/agentbox/project/cargo:/home/dev/.cargo"
        ));
        assert!(args.contains_option_from(
            SCCACHE_VOLUME_OWNER,
            "--volume",
            "/tmp/state/agentbox/sccache:/home/dev/.cache/sccache"
        ));
        assert!(args.contains_option_from(
            SCCACHE_VOLUME_OWNER,
            "--env",
            &format!("SCCACHE_DIR={CONTAINER_SCCACHE_DIR}")
        ));
        assert!(args.contains_option_from(
            SIDECAR_NIX_OWNER,
            "--volume",
            "/tmp/state/agentbox/project/nix-merged:/nix:ro"
        ));
        assert!(args.contains_option_from(
            SIDECAR_NIX_OWNER,
            "--env",
            &format!("NIX_REMOTE={NIX_REMOTE_SOCKET}")
        ));
        assert!(args.contains_option_from(
            SIDECAR_NIX_OWNER,
            "--label",
            &format!("{TASK_CONTAINER_ROLE_LABEL}={TASK_CONTAINER_ROLE_VALUE}")
        ));
        assert!(args.contains_option_from(
            SIDECAR_NIX_OWNER,
            "--label",
            &format!("{TASK_CONTAINER_SIDECAR_LABEL}=agentbox-nix-sidecar-abc")
        ));
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

    #[test]
    fn guest_diagnostics_envs_are_owned_by_guest_diagnostics() {
        let runtime = sidecar_runtime();
        let args = build_container_task_run_args(ContainerTaskPodmanSpec {
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

        assert!(args.contains_option_from(GUEST_DIAGNOSTICS_OWNER, "--env", GUEST_PROFILE_ENV));
        assert!(args.contains_option_from(GUEST_DIAGNOSTICS_OWNER, "--env", GUEST_DEBUG_ENV));
    }

    fn sidecar_runtime() -> SidecarNixRuntime {
        SidecarNixRuntime {
            merged_dir: PathBuf::from("/tmp/state/agentbox/project/nix-merged"),
            sidecar_name: "agentbox-nix-sidecar-abc".to_owned(),
            proxy_port: 19876,
            mount_mode: PodmanImageMountMode::Direct,
        }
    }

    fn build_args(nix_runtime: &SidecarNixRuntime) -> Vec<String> {
        build_container_task_podman_args(default_spec(nix_runtime))
            .expect("container task args should build")
    }

    fn build_run_args(nix_runtime: &SidecarNixRuntime) -> crate::podman::run::RunArgs {
        build_container_task_run_args(default_spec(nix_runtime))
            .expect("container task args should build")
    }

    fn default_spec(nix_runtime: &SidecarNixRuntime) -> ContainerTaskPodmanSpec<'_> {
        ContainerTaskPodmanSpec {
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
        }
    }

    fn default_expected_args() -> Vec<String> {
        [
            "run",
            "--rm",
            "-it",
            "--name",
            "project-random",
            "--userns",
            "keep-id",
            "--workdir",
            "/workspace",
            "--hostname",
            "project-agentbox",
            "--volume",
            "/tmp/project:/workspace",
            "--volume",
            "/home/alice/.codex:/home/dev/.codex",
            "--volume",
            "/tmp/state/agentbox/project/cargo:/home/dev/.cargo",
            "--volume",
            "/tmp/state/agentbox/sccache:/home/dev/.cache/sccache",
            "--env",
            "SCCACHE_DIR=/home/dev/.cache/sccache",
            "--tmpfs",
            "/tmp:rw,exec,mode=1777",
            "--volume",
            "/tmp/state/agentbox/project/nix-merged:/nix:ro",
            "--env",
            "NIX_REMOTE=unix:///nix/var/nix/daemon-socket/socket",
            "--label",
            "io.agentbox.role=task",
            "--label",
            "io.agentbox.sidecar=agentbox-nix-sidecar-abc",
            crate::DEFAULT_IMAGE,
            INTERACTIVE_SHELL,
            "-l",
        ]
        .map(str::to_owned)
        .to_vec()
    }
}
