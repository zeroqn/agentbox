use super::super::*;
use std::fs;
use std::path::Path;

#[test]
fn persistent_nix_root_is_project_local() {
    let root = PersistentNixRoot::new(Path::new("/tmp/project"));
    assert_eq!(root.root_dir(), Path::new("/tmp/project/.agentbox/nix"));
    assert_eq!(
        root.store_dir,
        Path::new("/tmp/project/.agentbox/nix/store")
    );
    assert_eq!(
        root.var_nix_dir,
        Path::new("/tmp/project/.agentbox/nix/var/nix")
    );
    assert_eq!(
        root.log_nix_dir,
        Path::new("/tmp/project/.agentbox/nix/var/log/nix")
    );
    assert_eq!(
        root.marker_file,
        Path::new("/tmp/project/.agentbox/nix/.seeded")
    );
}

#[test]
fn build_seed_podman_args_runs_seeding_as_root() {
    let args = build_seed_podman_args(
        DEFAULT_IMAGE,
        "/tmp/project/.agentbox/nix:/agentbox-nix",
        "echo seeding",
    );

    assert_eq!(
        args,
        vec![
            "run".to_owned(),
            "--rm".to_owned(),
            "--user".to_owned(),
            "0:0".to_owned(),
            "--volume".to_owned(),
            "/tmp/project/.agentbox/nix:/agentbox-nix".to_owned(),
            DEFAULT_IMAGE.to_owned(),
            "bash".to_owned(),
            "-lc".to_owned(),
            "echo seeding".to_owned(),
        ]
    );
}

#[test]
fn inspect_persistent_nix_root_reports_missing_for_empty_directories() {
    let dir = tempfile::tempdir().expect("tempdir should be created");
    let root = PersistentNixRoot::new(dir.path());
    assert_eq!(
        inspect_persistent_nix_root(&root).expect("state should inspect"),
        NixRootState::Missing
    );
}

#[test]
fn inspect_persistent_nix_root_reports_ready_when_marker_and_dirs_exist() {
    let dir = tempfile::tempdir().expect("tempdir should be created");
    let root = PersistentNixRoot::new(dir.path());
    fs::create_dir_all(&root.store_dir).expect("store dir should be created");
    fs::create_dir_all(&root.var_nix_dir).expect("var dir should be created");
    fs::write(&root.marker_file, "seeded").expect("marker should be written");
    assert_eq!(
        inspect_persistent_nix_root(&root).expect("state should inspect"),
        NixRootState::Ready
    );
}

#[test]
fn inspect_persistent_nix_root_reports_inconsistent_for_partial_state() {
    let dir = tempfile::tempdir().expect("tempdir should be created");
    let root = PersistentNixRoot::new(dir.path());
    fs::create_dir_all(&root.store_dir).expect("store dir should be created");
    fs::write(root.store_dir.join("placeholder"), "x").expect("placeholder should be written");
    assert_eq!(
        inspect_persistent_nix_root(&root).expect("state should inspect"),
        NixRootState::Inconsistent
    );
}

#[test]
fn ensure_persistent_nix_log_dir_creates_missing_path() {
    let dir = tempfile::tempdir().expect("tempdir should be created");
    let root = PersistentNixRoot::new(dir.path());
    ensure_persistent_nix_log_dir(&root).expect("log dir should be created");
    assert!(root.log_nix_dir.is_dir());
}

#[test]
fn seed_script_targets_persistent_nix_root_layout() {
    let script = build_seed_script(false);
    assert!(script.contains("mkdir -p /agentbox-nix/store"));
    assert!(script.contains("mkdir -p /agentbox-nix/var/nix"));
    assert!(script.contains("mkdir -p /agentbox-nix/var/log/nix"));
    assert!(script.contains("cp -a /nix/store/. /agentbox-nix/store/"));
    assert!(script.contains("cp -a /nix/var/nix/. /agentbox-nix/var/nix/"));
    assert!(!script.contains("find /agentbox-nix -mindepth 1 -maxdepth 1 -exec rm -rf -- {} +"));
}

#[test]
fn persistent_nix_root_seed_mount_targets_nix_subdirectory() {
    let root = PersistentNixRoot::new(Path::new("/tmp/project"));
    let seed_mount = format_mount_arg(root.root_dir(), SEED_MOUNT_POINT)
        .expect("seed mount formatting should succeed");
    assert_eq!(seed_mount, "/tmp/project/.agentbox/nix:/agentbox-nix");
}

#[test]
fn forced_seed_script_clears_existing_contents_inside_container() {
    let script = build_seed_script(true);
    assert!(script.contains("find /agentbox-nix -mindepth 1 -maxdepth 1 -exec rm -rf -- {} +"));
    assert!(script.contains("mkdir -p /agentbox-nix/store"));
}
