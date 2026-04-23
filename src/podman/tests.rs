use super::task::build_podman_args;
use crate::*;
use std::path::PathBuf;

#[test]
fn build_podman_args_includes_persistent_nix_mounts() {
    let root = PersistentNixRoot::new(std::path::Path::new("/tmp/state/agentbox/project"));
    let runtime = NixRuntime::Seeded(root);
    let args = build_podman_args(
        DEFAULT_IMAGE,
        "project-agentbox",
        "agentbox-task-project-1234",
        "project",
        "/tmp/project:/workspace",
        "/home/alice/.codex:/home/dev/.codex",
        "/tmp/state/agentbox/project/cargo:/home/dev/.cargo",
        "/tmp/state/agentbox/sccache:/home/dev/.cache/sccache",
        &runtime,
    )
    .expect("podman args should build");
    assert_eq!(args[3], "--userns");
    assert_eq!(args[4], "keep-id");
    assert!(args.contains(&"--hostname".to_owned()));
    assert!(args.contains(&"project-agentbox".to_owned()));
    assert!(args.contains(&"--name".to_owned()));
    assert!(args.contains(&"agentbox-task-project-1234".to_owned()));
    assert!(args.contains(&format!("{TASK_CONTAINER_WORKSPACE_LABEL}=project")));
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
}

#[test]
fn build_podman_args_includes_sidecar_nix_mount_and_remote() {
    let runtime = NixRuntime::Sidecar(SidecarNixRuntime {
        merged_dir: PathBuf::from("/tmp/state/agentbox/project/nix-merged"),
        sidecar_name: "agentbox-nix-sidecar-abc".to_owned(),
    });
    let args = build_podman_args(
        DEFAULT_IMAGE,
        "project-agentbox",
        "agentbox-task-project-1234",
        "project",
        "/tmp/project:/workspace",
        "/home/alice/.codex:/home/dev/.codex",
        "/tmp/state/agentbox/project/cargo:/home/dev/.cargo",
        "/tmp/state/agentbox/sccache:/home/dev/.cache/sccache",
        &runtime,
    )
    .expect("podman args should build");

    assert!(args.contains(&"/tmp/state/agentbox/project/nix-merged:/nix:ro".to_owned()));
    assert!(args.contains(&"/tmp/state/agentbox/sccache:/home/dev/.cache/sccache".to_owned()));
    assert!(args.contains(&"--hostname".to_owned()));
    assert!(args.contains(&"project-agentbox".to_owned()));
    assert!(args.contains(&"--name".to_owned()));
    assert!(args.contains(&"agentbox-task-project-1234".to_owned()));
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
    assert!(args.contains(&format!("{TASK_CONTAINER_WORKSPACE_LABEL}=project")));
    assert!(!args.contains(&"/tmp/state/agentbox/project/nix/store:/nix/store".to_owned()));
    assert!(!args.contains(&"/tmp/state/agentbox/project/nix/var/nix:/nix/var/nix".to_owned()));
}
