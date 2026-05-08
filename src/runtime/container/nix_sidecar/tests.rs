use super::*;
use std::fs;
use std::path::{Path, PathBuf};

#[test]
fn sidecar_paths_use_resolved_state_root() {
    let paths = SidecarPaths::new(Path::new("/tmp/state/agentbox/project"));
    assert_eq!(
        paths.upper_dir,
        Path::new("/tmp/state/agentbox/project/nix-upper")
    );
    assert_eq!(
        paths.work_dir,
        Path::new("/tmp/state/agentbox/project/nix-work")
    );
    assert_eq!(
        paths.merged_dir,
        Path::new("/tmp/state/agentbox/project/nix-merged")
    );
    assert_eq!(
        paths.state_file,
        Path::new("/tmp/state/agentbox/project/nix-sidecar.state")
    );
}

#[test]
fn sidecar_name_is_deterministic_for_same_workspace_and_image_id() {
    let cwd = Path::new("/tmp/project");
    let image_id = "sha256:abc123";
    let first = name::derive_sidecar_name(cwd, image_id);
    let second = name::derive_sidecar_name(cwd, image_id);
    let third = name::derive_sidecar_name(cwd, "sha256:def456");

    assert_eq!(first, second);
    assert_ne!(first, third);
    assert!(first.starts_with("agentbox-nix-sidecar-project-"));
}

#[test]
fn sidecar_name_sanitizes_workspace_name_into_slug() {
    let cwd = Path::new("/tmp/My repo.name!");
    let sidecar_name = name::derive_sidecar_name(cwd, "sha256:abc123");

    assert!(sidecar_name.starts_with("agentbox-nix-sidecar-my-repo-name-"));
}

#[test]
fn sidecar_name_falls_back_when_workspace_name_has_no_slug_chars() {
    let cwd = Path::new("/tmp/!!!");
    let sidecar_name = name::derive_sidecar_name(cwd, "sha256:abc123");

    assert!(sidecar_name.starts_with("agentbox-nix-sidecar-workspace-"));
}

#[test]
fn sidecar_state_round_trip_via_state_file() {
    let dir = tempfile::tempdir().expect("tempdir should be created");
    let paths = SidecarPaths::new(&dir.path().join("state").join("agentbox").join("project"));
    let state = SidecarState {
        image: crate::DEFAULT_IMAGE.to_owned(),
        image_id: "sha256:abc123".to_owned(),
        image_mount_path: PathBuf::from("/tmp/podman/mounts/abc"),
        sidecar_name: "agentbox-nix-sidecar-abc".to_owned(),
        mount_mode: PodmanImageMountMode::Unshare,
        proxy_port: Some(12345),
        native_config: true,
    };

    state::write_sidecar_state(&paths, &state).expect("state should be written");
    let contents = fs::read_to_string(&paths.state_file).expect("state should be readable");
    assert!(!contents.contains("runtime_mode="));
    assert!(!contents.contains("network_mode="));

    let parsed = state::read_sidecar_state(&paths)
        .expect("state should parse")
        .expect("state should exist");

    assert_eq!(parsed.image, state.image);
    assert_eq!(parsed.image_id, state.image_id);
    assert_eq!(parsed.image_mount_path, state.image_mount_path);
    assert_eq!(parsed.sidecar_name, state.sidecar_name);
    assert_eq!(parsed.mount_mode, state.mount_mode);
    assert_eq!(parsed.proxy_port, state.proxy_port);
    assert!(parsed.native_config);
}

#[test]
fn sidecar_state_without_mount_or_runtime_mode_defaults_to_direct_native() {
    let dir = tempfile::tempdir().expect("tempdir should be created");
    let paths = SidecarPaths::new(&dir.path().join("state").join("agentbox").join("project"));
    fs::create_dir_all(
        paths
            .state_file
            .parent()
            .expect("state file should have parent directory"),
    )
    .expect("state directory should be created");
    fs::write(
        &paths.state_file,
        "image=localhost/agentbox:latest\nimage_id=sha256:abc\nimage_mount_path=/tmp/podman/mount\nsidecar_name=agentbox-nix-sidecar-abc\n",
    )
    .expect("legacy state should be written");

    let parsed = state::read_sidecar_state(&paths)
        .expect("state should parse")
        .expect("state should exist");
    assert_eq!(parsed.mount_mode, PodmanImageMountMode::Direct);
    assert!(parsed.native_config);
}

#[test]
fn legacy_libkrun_state_parses_as_non_native_config() {
    let dir = tempfile::tempdir().expect("tempdir should be created");
    let paths = SidecarPaths::new(&dir.path().join("state").join("agentbox").join("project"));
    fs::create_dir_all(
        paths
            .state_file
            .parent()
            .expect("state file should have parent directory"),
    )
    .expect("state directory should be created");
    fs::write(
        &paths.state_file,
        "image=localhost/agentbox:latest\nimage_id=sha256:abc\nimage_mount_path=/tmp/podman/mount\nsidecar_name=agentbox-nix-sidecar-abc\nruntime_mode=libkrun\nnetwork_mode=tsi\n",
    )
    .expect("legacy state should be written");

    let parsed = state::read_sidecar_state(&paths)
        .expect("state should parse")
        .expect("state should exist");
    assert!(!parsed.native_config);
}

#[test]
fn stale_incomplete_sidecar_state_is_auto_cleared() {
    let dir = tempfile::tempdir().expect("tempdir should be created");
    let paths = SidecarPaths::new(&dir.path().join("state").join("agentbox").join("project"));
    fs::create_dir_all(
        paths
            .state_file
            .parent()
            .expect("state file should have parent directory"),
    )
    .expect("state directory should be created");
    fs::write(&paths.state_file, "image=localhost/agentbox:latest\n")
        .expect("stale state should be written");

    let parsed = state::read_sidecar_state(&paths).expect("state read should succeed");
    assert!(parsed.is_none(), "stale state should be ignored");
    assert!(
        !paths.state_file.exists(),
        "stale state file should be removed automatically"
    );
}

#[test]
fn invalid_mount_mode_state_is_auto_cleared() {
    let dir = tempfile::tempdir().expect("tempdir should be created");
    let paths = SidecarPaths::new(&dir.path().join("state").join("agentbox").join("project"));
    fs::create_dir_all(
        paths
            .state_file
            .parent()
            .expect("state file should have parent directory"),
    )
    .expect("state directory should be created");
    fs::write(
        &paths.state_file,
        "image=localhost/agentbox:latest\nimage_id=sha256:abc\nimage_mount_path=/tmp/podman/mount\nsidecar_name=agentbox-nix-sidecar-abc\nmount_mode=unsupported\n",
    )
    .expect("invalid state should be written");

    let parsed = state::read_sidecar_state(&paths).expect("state read should succeed");
    assert!(
        parsed.is_none(),
        "invalid mount mode state should be ignored"
    );
    assert!(
        !paths.state_file.exists(),
        "invalid state file should be removed automatically"
    );
}

#[test]
fn invalid_proxy_port_state_is_auto_cleared() {
    let dir = tempfile::tempdir().expect("tempdir should be created");
    let paths = SidecarPaths::new(&dir.path().join("state").join("agentbox").join("project"));
    fs::create_dir_all(
        paths
            .state_file
            .parent()
            .expect("state file should have parent directory"),
    )
    .expect("state directory should be created");
    fs::write(
        &paths.state_file,
        "image=localhost/agentbox:latest\nimage_id=sha256:abc\nimage_mount_path=/tmp/podman/mount\nsidecar_name=agentbox-nix-sidecar-abc\nproxy_port=not-a-port\n",
    )
    .expect("invalid state should be written");

    let parsed = state::read_sidecar_state(&paths).expect("state read should succeed");
    assert!(
        parsed.is_none(),
        "invalid proxy port state should be ignored"
    );
    assert!(
        !paths.state_file.exists(),
        "invalid state file should be removed automatically"
    );
}
