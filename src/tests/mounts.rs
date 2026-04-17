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
fn prepare_project_cargo_mount_creates_agentbox_cargo_under_workspace() {
    let dir = tempfile::tempdir().expect("tempdir should be created");

    let mount = prepare_project_cargo_mount(dir.path()).expect("cargo mount should be prepared");

    assert_eq!(
        mount,
        format!(
            "{}:{CONTAINER_CARGO_DIR}",
            dir.path().join(".agentbox").join("cargo").display()
        )
    );
    assert!(dir.path().join(".agentbox").join("cargo").is_dir());
}
