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
    assert!(!args.contains(&"--runtime".to_owned()));
    assert!(!args.contains(&"run.oci.handler=krun".to_owned()));
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
    assert!(!args.contains(&"--runtime".to_owned()));
    assert!(!args.contains(&"run.oci.handler=krun".to_owned()));
    assert_eq!(args[args.len() - 2], INTERACTIVE_SHELL);
    assert_eq!(args[args.len() - 1], "-l");
}

#[test]
fn build_podman_args_adds_only_krun_runtime_args_for_kvm_task_mode() {
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

    let krun_args = [
        "--runtime".to_owned(),
        "crun".to_owned(),
        "--annotation".to_owned(),
        "run.oci.handler=krun".to_owned(),
    ];
    let krun_index = kvm_args
        .windows(krun_args.len())
        .position(|window| window == krun_args)
        .expect("kvm args should include contiguous krun runtime args");
    let mut kvm_without_krun_args = kvm_args;
    kvm_without_krun_args.drain(krun_index..krun_index + krun_args.len());

    assert_eq!(kvm_without_krun_args, native_args);
}
