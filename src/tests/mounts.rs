use super::super::*;
use std::path::Path;

#[test]
fn mount_arg_uses_workspace_path() {
    let mount = format_mount_arg(Path::new("/tmp/project"), CONTAINER_WORKDIR)
        .expect("mount formatting should succeed");
    assert_eq!(mount, "/tmp/project:/workspace");
}

#[test]
fn mount_arg_supports_options_suffix() {
    let mount = format_mount_arg_with_options(Path::new("/tmp/project"), "/nix", Some("ro"))
        .expect("mount formatting should succeed");
    assert_eq!(mount, "/tmp/project:/nix:ro");
}

#[test]
fn prepare_host_codex_mount_creates_dot_codex_under_home() {
    let dir = tempfile::tempdir().expect("tempdir should be created");

    let mount = prepare_host_codex_mount_at(dir.path()).expect("codex mount should be prepared");

    assert_eq!(
        mount,
        format!(
            "{}:{CONTAINER_CODEX_DIR}",
            dir.path().join(".codex").display()
        )
    );
    assert!(dir.path().join(".codex").is_dir());
}

#[test]
fn prepare_project_cargo_mount_creates_cargo_under_state_root() {
    let dir = tempfile::tempdir().expect("tempdir should be created");
    let state_root = dir.path().join("state").join("agentbox").join("project");

    let mount = prepare_project_cargo_mount(&state_root).expect("cargo mount should be prepared");

    assert_eq!(
        mount,
        format!(
            "{}:{CONTAINER_CARGO_DIR}",
            state_root.join("cargo").display()
        )
    );
    assert!(state_root.join("cargo").is_dir());
}
