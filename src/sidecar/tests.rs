use super::*;
use crate::*;
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
fn resolve_sidecar_lowerdir_prefers_nested_nix_directory() {
    let dir = tempfile::tempdir().expect("tempdir should be created");
    let mount = dir.path();
    fs::create_dir_all(mount.join("nix")).expect("nested nix dir should be created");
    fs::create_dir_all(mount.join("store")).expect("root store dir should be created");

    let lowerdir = resolve_sidecar_lowerdir(mount).expect("lowerdir should resolve");
    assert_eq!(lowerdir, mount.join("nix"));
}

#[test]
fn resolve_sidecar_lowerdir_falls_back_to_mount_root_when_store_exists() {
    let dir = tempfile::tempdir().expect("tempdir should be created");
    let mount = dir.path();
    fs::create_dir_all(mount.join("store")).expect("root store dir should be created");

    let lowerdir = resolve_sidecar_lowerdir(mount).expect("lowerdir should resolve");
    assert_eq!(lowerdir, mount);
}

#[test]
fn resolve_sidecar_lowerdir_fails_when_nix_paths_are_missing() {
    let dir = tempfile::tempdir().expect("tempdir should be created");
    let mount = dir.path();

    let err = resolve_sidecar_lowerdir(mount).expect_err("lowerdir should fail");
    assert!(err
        .to_string()
        .contains(&mount.join("nix").display().to_string()));
    assert!(err
        .to_string()
        .contains(&mount.join("store").display().to_string()));
}

#[test]
fn sidecar_name_is_deterministic_for_same_workspace_and_image_id() {
    let cwd = Path::new("/tmp/project");
    let image_id = "sha256:abc123";
    let first = derive_sidecar_name(cwd, image_id);
    let second = derive_sidecar_name(cwd, image_id);
    let third = derive_sidecar_name(cwd, "sha256:def456");

    assert_eq!(first, second);
    assert_ne!(first, third);
    assert!(first.starts_with("agentbox-nix-sidecar-project-"));
}

#[test]
fn sidecar_name_sanitizes_workspace_name_into_slug() {
    let cwd = Path::new("/tmp/My repo.name!");
    let sidecar_name = derive_sidecar_name(cwd, "sha256:abc123");

    assert!(sidecar_name.starts_with("agentbox-nix-sidecar-my-repo-name-"));
}

#[test]
fn sidecar_name_falls_back_when_workspace_name_has_no_slug_chars() {
    let cwd = Path::new("/tmp/!!!");
    let sidecar_name = derive_sidecar_name(cwd, "sha256:abc123");

    assert!(sidecar_name.starts_with("agentbox-nix-sidecar-workspace-"));
}

#[test]
fn build_sidecar_task_probe_args_filters_for_task_role_and_sidecar_name() {
    let args = build_sidecar_task_probe_args("agentbox-nix-sidecar-abc");
    assert_eq!(
        args,
        vec![
            "ps".to_owned(),
            "--filter".to_owned(),
            format!("label={TASK_CONTAINER_ROLE_LABEL}={TASK_CONTAINER_ROLE_VALUE}"),
            "--filter".to_owned(),
            format!("label={TASK_CONTAINER_SIDECAR_LABEL}=agentbox-nix-sidecar-abc"),
            "--format".to_owned(),
            "{{.ID}}".to_owned(),
        ]
    );
}

#[test]
fn build_sidecar_podman_args_runs_daemon_as_root_and_mounts_rw_nix() {
    let args = build_sidecar_podman_args(
        DEFAULT_IMAGE,
        "agentbox-nix-sidecar-abc",
        "/tmp/state/agentbox/project/nix-merged:/nix",
    );

    assert_eq!(args[0], "run");
    assert!(args.contains(&"-d".to_owned()));
    assert!(!args.contains(&"--rm".to_owned()));
    assert!(args.contains(&"--name".to_owned()));
    assert!(args.contains(&"agentbox-nix-sidecar-abc".to_owned()));
    assert!(args.contains(&"--user".to_owned()));
    assert!(args.contains(&"0:0".to_owned()));
    assert!(args.contains(&"--volume".to_owned()));
    assert!(args.contains(&"/tmp/state/agentbox/project/nix-merged:/nix".to_owned()));
    assert_eq!(args[args.len() - 3], "bash");
    assert_eq!(args[args.len() - 2], "-lc");
    assert!(args[args.len() - 1].contains("nix-daemon --daemon"));
}

#[test]
fn build_socket_ping_podman_args_targets_nix_remote_socket() {
    let args = build_socket_ping_podman_args(
        DEFAULT_IMAGE,
        "/tmp/state/agentbox/project/nix-merged:/nix:ro",
    );

    assert!(args.contains(&"--userns".to_owned()));
    assert!(args.contains(&"keep-id".to_owned()));
    assert!(args.contains(&"/tmp/state/agentbox/project/nix-merged:/nix:ro".to_owned()));
    assert_eq!(
        args[args.len() - 1],
        format!("nix store ping --store {NIX_REMOTE_SOCKET}")
    );
}

#[test]
fn sidecar_socket_timeout_error_includes_auto_cleanup_and_log_tail() {
    let merged_dir = Path::new("/tmp/state/agentbox/project/nix-merged");
    let cleanup_outcome = SidecarStartupCleanupOutcome {
        summary: "removed sidecar 'agentbox-nix-sidecar-abc' (or it was already absent); cleaned merged mount '/tmp/state/agentbox/project/nix-merged'".to_owned(),
        manual_merged_cleanup_required: false,
    };
    let diagnostics = SidecarStartupDiagnostics {
        sidecar_logs: Some("daemon booting\nready".to_owned()),
        sidecar_logs_error: None,
        socket_probe_failure: Some("probe exited with status 1".to_owned()),
        sidecar_state: Some("running=false status=exited exit_code=1".to_owned()),
        host_socket_exists: Some(false),
    };

    let message = build_sidecar_socket_timeout_error(
        "agentbox-nix-sidecar-abc",
        merged_dir,
        &cleanup_outcome,
        &diagnostics,
    );

    assert!(message.contains("Automatic cleanup completed"));
    assert!(message.contains("/tmp/state/agentbox/project/nix-merged"));
    assert!(message.contains("recent sidecar logs:\ndaemon booting\nready"));
    assert!(message.contains("sidecar state: running=false status=exited exit_code=1"));
    assert!(message.contains("socket probe failure: probe exited with status 1"));
    assert!(message.contains("host socket path exists: no"));
}

#[test]
fn sidecar_socket_timeout_error_requests_manual_cleanup_when_auto_cleanup_fails() {
    let merged_dir = Path::new("/tmp/state/agentbox/project/nix-merged");
    let cleanup_outcome = SidecarStartupCleanupOutcome {
        summary: "failed to remove sidecar 'agentbox-nix-sidecar-abc': boom".to_owned(),
        manual_merged_cleanup_required: true,
    };
    let diagnostics = SidecarStartupDiagnostics {
        sidecar_logs: None,
        sidecar_logs_error: Some("logs missing".to_owned()),
        socket_probe_failure: None,
        sidecar_state: None,
        host_socket_exists: Some(true),
    };

    let message = build_sidecar_socket_timeout_error(
        "agentbox-nix-sidecar-abc",
        merged_dir,
        &cleanup_outcome,
        &diagnostics,
    );

    assert!(message.contains("could not be cleaned automatically"));
    assert!(message.contains("remove it before retrying"));
    assert!(message.contains("sidecar logs unavailable (logs missing)"));
    assert!(message.contains("host socket path exists: yes"));
}

#[test]
fn build_podman_image_mount_args_uses_direct_mode_by_default_path() {
    let args = build_podman_image_mount_args(DEFAULT_IMAGE, PodmanImageMountMode::Direct);
    assert_eq!(
        args,
        vec![
            "image".to_owned(),
            "mount".to_owned(),
            DEFAULT_IMAGE.to_owned(),
        ]
    );
}

#[test]
fn build_podman_image_mount_args_supports_unshare_fallback_mode() {
    let args = build_podman_image_mount_args(DEFAULT_IMAGE, PodmanImageMountMode::Unshare);
    assert_eq!(
        args,
        vec![
            "unshare".to_owned(),
            "podman".to_owned(),
            "image".to_owned(),
            "mount".to_owned(),
            DEFAULT_IMAGE.to_owned(),
        ]
    );
}

#[test]
fn build_podman_image_unmount_args_uses_direct_mode_by_default_path() {
    let args = build_podman_image_unmount_args(DEFAULT_IMAGE, PodmanImageMountMode::Direct);
    assert_eq!(
        args,
        vec![
            "image".to_owned(),
            "unmount".to_owned(),
            DEFAULT_IMAGE.to_owned(),
        ]
    );
}

#[test]
fn build_podman_image_unmount_args_supports_unshare_fallback_mode() {
    let args = build_podman_image_unmount_args(DEFAULT_IMAGE, PodmanImageMountMode::Unshare);
    assert_eq!(
        args,
        vec![
            "unshare".to_owned(),
            "podman".to_owned(),
            "image".to_owned(),
            "unmount".to_owned(),
            DEFAULT_IMAGE.to_owned(),
        ]
    );
}

#[test]
fn sidecar_state_round_trip_via_state_file() {
    let dir = tempfile::tempdir().expect("tempdir should be created");
    let paths = SidecarPaths::new(&dir.path().join("state").join("agentbox").join("project"));
    let state = SidecarState {
        image: DEFAULT_IMAGE.to_owned(),
        image_id: "sha256:abc123".to_owned(),
        image_mount_path: PathBuf::from("/tmp/podman/mounts/abc"),
        sidecar_name: "agentbox-nix-sidecar-abc".to_owned(),
        mount_mode: PodmanImageMountMode::Unshare,
    };

    write_sidecar_state(&paths, &state).expect("state should be written");
    let parsed = read_sidecar_state(&paths)
        .expect("state should parse")
        .expect("state should exist");

    assert_eq!(parsed.image, state.image);
    assert_eq!(parsed.image_id, state.image_id);
    assert_eq!(parsed.image_mount_path, state.image_mount_path);
    assert_eq!(parsed.sidecar_name, state.sidecar_name);
    assert_eq!(parsed.mount_mode, state.mount_mode);
}

#[test]
fn sidecar_state_without_mount_mode_defaults_to_direct() {
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

    let parsed = read_sidecar_state(&paths)
        .expect("state should parse")
        .expect("state should exist");
    assert_eq!(parsed.mount_mode, PodmanImageMountMode::Direct);
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

    let parsed = read_sidecar_state(&paths).expect("state read should succeed");
    assert!(parsed.is_none(), "stale state should be ignored");
    assert!(
        !paths.state_file.exists(),
        "stale state file should be removed automatically"
    );
}
