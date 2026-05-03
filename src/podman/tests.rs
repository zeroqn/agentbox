use super::task::{build_podman_args, TaskPodmanSpec};
use crate::*;
use std::path::PathBuf;

#[test]
fn build_podman_args_includes_persistent_nix_mounts() {
    let root = PersistentNixRoot::new(std::path::Path::new("/tmp/state/agentbox/project"));
    let runtime = NixRuntime::Seeded(root);
    let args = build_podman_args(TaskPodmanSpec {
        image: DEFAULT_IMAGE,
        hostname: "project-agentbox",
        workspace_mount: "/tmp/project:/workspace",
        codex_mount: "/home/alice/.codex:/home/dev/.codex",
        cargo_mount: "/tmp/state/agentbox/project/cargo:/home/dev/.cargo",
        sccache_mount: "/tmp/state/agentbox/sccache:/home/dev/.cache/sccache",
        nix_runtime: &runtime,
        task_mode: TaskContainerMode::Native,
    })
    .expect("podman args should build");
    assert_eq!(args[3], "--userns");
    assert_eq!(args[4], "keep-id");
    assert!(args.contains(&"--hostname".to_owned()));
    assert!(args.contains(&"project-agentbox".to_owned()));
    assert!(args.contains(&"/tmp/state/agentbox/project/nix/store:/nix/store".to_owned()));
    assert!(args.contains(&"/tmp/state/agentbox/project/nix/var/nix:/nix/var/nix".to_owned()));
    assert!(
        args.contains(&"/tmp/state/agentbox/project/nix/var/log/nix:/nix/var/log/nix".to_owned())
    );
    assert!(args.contains(&"/home/alice/.codex:/home/dev/.codex".to_owned()));
    assert!(args.contains(&"/tmp/state/agentbox/project/cargo:/home/dev/.cargo".to_owned()));
    assert!(args.contains(&"/tmp/state/agentbox/sccache:/home/dev/.cache/sccache".to_owned()));
    assert!(args.contains(&"--tmpfs".to_owned()));
    assert!(args.contains(&CONTAINER_TMP_TMPFS.to_owned()));
    assert!(args.contains(&"--env".to_owned()));
    assert!(args.contains(&format!("SCCACHE_DIR={CONTAINER_SCCACHE_DIR}")));
    assert_eq!(args[args.len() - 2], INTERACTIVE_SHELL);
    assert_eq!(args[args.len() - 1], "-l");
    assert!(!args.contains(&"--user".to_owned()));
    assert!(!args.contains(&format!("NIX_REMOTE={NIX_REMOTE_SOCKET}")));
    assert!(!args.contains(&TASK_KVM_DROP_TO_DEV_ENV.to_owned()));
    assert!(!args.contains(&"--runtime".to_owned()));
    assert!(!args.contains(&"run.oci.handler=krun".to_owned()));
    assert!(!args.iter().any(|a| a.starts_with(HOST_UID_ENV_PREFIX)));
    assert!(!args.iter().any(|a| a.starts_with(HOST_GID_ENV_PREFIX)));
}

#[test]
fn build_podman_args_includes_sidecar_nix_mount_and_remote() {
    let runtime = NixRuntime::Sidecar(SidecarNixRuntime {
        merged_dir: PathBuf::from("/tmp/state/agentbox/project/nix-merged"),
        sidecar_name: "agentbox-nix-sidecar-abc".to_owned(),
    });
    let args = build_podman_args(TaskPodmanSpec {
        image: DEFAULT_IMAGE,
        hostname: "project-agentbox",
        workspace_mount: "/tmp/project:/workspace",
        codex_mount: "/home/alice/.codex:/home/dev/.codex",
        cargo_mount: "/tmp/state/agentbox/project/cargo:/home/dev/.cargo",
        sccache_mount: "/tmp/state/agentbox/sccache:/home/dev/.cache/sccache",
        nix_runtime: &runtime,
        task_mode: TaskContainerMode::Native,
    })
    .expect("podman args should build");

    assert!(args.contains(&"/tmp/state/agentbox/project/nix-merged:/nix:ro".to_owned()));
    assert!(args.contains(&"/tmp/state/agentbox/sccache:/home/dev/.cache/sccache".to_owned()));
    assert!(args.contains(&"--hostname".to_owned()));
    assert!(args.contains(&"project-agentbox".to_owned()));
    assert!(args.contains(&"--env".to_owned()));
    assert!(args.contains(&format!("SCCACHE_DIR={CONTAINER_SCCACHE_DIR}")));
    assert!(args.contains(&format!("NIX_REMOTE={NIX_REMOTE_SOCKET}")));
    assert!(args.contains(&"--label".to_owned()));
    assert!(args.contains(&format!(
        "{TASK_CONTAINER_ROLE_LABEL}={TASK_CONTAINER_ROLE_VALUE}"
    )));
    assert!(args.contains(&format!(
        "{TASK_CONTAINER_SIDECAR_LABEL}=agentbox-nix-sidecar-abc"
    )));
    assert!(!args.contains(&"/tmp/state/agentbox/project/nix/store:/nix/store".to_owned()));
    assert!(!args.contains(&"/tmp/state/agentbox/project/nix/var/nix:/nix/var/nix".to_owned()));
    assert!(!args.contains(&TASK_KVM_DROP_TO_DEV_ENV.to_owned()));
    assert!(!args.contains(&"--runtime".to_owned()));
    assert!(!args.contains(&"run.oci.handler=krun".to_owned()));
    assert!(!args.iter().any(|a| a.starts_with(HOST_UID_ENV_PREFIX)));
    assert!(!args.iter().any(|a| a.starts_with(HOST_GID_ENV_PREFIX)));
    assert_eq!(args[args.len() - 2], INTERACTIVE_SHELL);
    assert_eq!(args[args.len() - 1], "-l");
}

#[test]
fn build_podman_args_adds_only_kvm_runtime_args_for_kvm_task_mode() {
    let runtime = NixRuntime::Sidecar(SidecarNixRuntime {
        merged_dir: PathBuf::from("/tmp/state/agentbox/project/nix-merged"),
        sidecar_name: "agentbox-nix-sidecar-abc".to_owned(),
    });
    let native_args = build_podman_args(TaskPodmanSpec {
        image: DEFAULT_IMAGE,
        hostname: "project-agentbox",
        workspace_mount: "/tmp/project:/workspace",
        codex_mount: "/home/alice/.codex:/home/dev/.codex",
        cargo_mount: "/tmp/state/agentbox/project/cargo:/home/dev/.cargo",
        sccache_mount: "/tmp/state/agentbox/sccache:/home/dev/.cache/sccache",
        nix_runtime: &runtime,
        task_mode: TaskContainerMode::Native,
    })
    .expect("native podman args should build");
    let kvm_args = build_podman_args(TaskPodmanSpec {
        image: DEFAULT_IMAGE,
        hostname: "project-agentbox",
        workspace_mount: "/tmp/project:/workspace",
        codex_mount: "/home/alice/.codex:/home/dev/.codex",
        cargo_mount: "/tmp/state/agentbox/project/cargo:/home/dev/.cargo",
        sccache_mount: "/tmp/state/agentbox/sccache:/home/dev/.cache/sccache",
        nix_runtime: &runtime,
        task_mode: TaskContainerMode::KvmKrunExperimental,
    })
    .expect("kvm podman args should build");

    // Verify host UID/GID env vars are present in KVM mode
    let has_host_uid = kvm_args
        .windows(2)
        .any(|w| w[0] == "--env" && w[1].starts_with(HOST_UID_ENV_PREFIX));
    assert!(
        has_host_uid,
        "kvm args should include AGENTBOX_HOST_UID env var"
    );
    let has_host_gid = kvm_args
        .windows(2)
        .any(|w| w[0] == "--env" && w[1].starts_with(HOST_GID_ENV_PREFIX));
    assert!(
        has_host_gid,
        "kvm args should include AGENTBOX_HOST_GID env var"
    );

    // Find and drain the KVM-specific argument block (host UID/GID env vars
    // are dynamic so we locate by the fixed start/end markers).
    let kvm_block_start = kvm_args
        .windows(2)
        .position(|w| w[0] == "--env" && w[1] == TASK_KVM_DROP_TO_DEV_ENV)
        .expect("kvm args should include AGENTBOX_KVM_DROP_TO_DEV env");
    let annotation_end = kvm_args
        .iter()
        .rposition(|a| a == "run.oci.handler=krun")
        .expect("kvm args should include run.oci.handler=krun annotation");

    let mut kvm_without_kvm_args = kvm_args;
    kvm_without_kvm_args.drain(kvm_block_start..=annotation_end);

    assert_eq!(kvm_without_kvm_args, native_args);
}
