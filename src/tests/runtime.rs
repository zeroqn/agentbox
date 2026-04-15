use super::super::*;
use std::fs;
use std::path::{Path, PathBuf};

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
fn sidecar_paths_are_project_local() {
    let paths = SidecarPaths::new(Path::new("/tmp/project"));
    assert_eq!(
        paths.upper_dir,
        Path::new("/tmp/project/.agentbox/nix-upper")
    );
    assert_eq!(paths.work_dir, Path::new("/tmp/project/.agentbox/nix-work"));
    assert_eq!(
        paths.merged_dir,
        Path::new("/tmp/project/.agentbox/nix-merged")
    );
    assert_eq!(
        paths.state_file,
        Path::new("/tmp/project/.agentbox/nix-sidecar.state")
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
    assert!(first.starts_with("agentbox-nix-sidecar-"));
}

#[test]
fn build_podman_args_includes_persistent_nix_mounts() {
    let root = PersistentNixRoot::new(Path::new("/tmp/project"));
    let runtime = NixRuntime::Seeded(root);
    let args = build_podman_args(
        DEFAULT_IMAGE,
        "/tmp/project:/workspace",
        "/home/alice/.codex:/home/dev/.codex",
        "/tmp/project/.agentbox/cargo:/home/dev/.cargo",
        &runtime,
    )
    .expect("podman args should build");
    assert_eq!(args[3], "--userns");
    assert_eq!(args[4], "keep-id");
    assert!(args.contains(&"/tmp/project/.agentbox/nix/store:/nix/store".to_owned()));
    assert!(args.contains(&"/tmp/project/.agentbox/nix/var/nix:/nix/var/nix".to_owned()));
    assert!(args.contains(&"/tmp/project/.agentbox/nix/var/log/nix:/nix/var/log/nix".to_owned()));
    assert!(args.contains(&"/home/alice/.codex:/home/dev/.codex".to_owned()));
    assert!(args.contains(&"/tmp/project/.agentbox/cargo:/home/dev/.cargo".to_owned()));
    assert!(args.contains(&"--tmpfs".to_owned()));
    assert!(args.contains(&CONTAINER_TMP_TMPFS.to_owned()));
    assert_eq!(args[args.len() - 2], INTERACTIVE_SHELL);
    assert_eq!(args[args.len() - 1], "-l");
    assert!(!args.contains(&"--user".to_owned()));
    assert!(!args.contains(&"--env".to_owned()));
}

#[test]
fn build_podman_args_includes_sidecar_nix_mount_and_remote() {
    let runtime = NixRuntime::Sidecar(SidecarNixRuntime {
        merged_dir: PathBuf::from("/tmp/project/.agentbox/nix-merged"),
        sidecar_name: "agentbox-nix-sidecar-abc".to_owned(),
    });
    let args = build_podman_args(
        DEFAULT_IMAGE,
        "/tmp/project:/workspace",
        "/home/alice/.codex:/home/dev/.codex",
        "/tmp/project/.agentbox/cargo:/home/dev/.cargo",
        &runtime,
    )
    .expect("podman args should build");

    assert!(args.contains(&"/tmp/project/.agentbox/nix-merged:/nix:ro".to_owned()));
    assert!(args.contains(&"--env".to_owned()));
    assert!(args.contains(&format!("NIX_REMOTE={NIX_REMOTE_SOCKET}")));
    assert!(args.contains(&"--label".to_owned()));
    assert!(args.contains(&format!(
        "{TASK_CONTAINER_ROLE_LABEL}={TASK_CONTAINER_ROLE_VALUE}"
    )));
    assert!(args.contains(&format!(
        "{TASK_CONTAINER_SIDECAR_LABEL}=agentbox-nix-sidecar-abc"
    )));
    assert!(!args.contains(&"/tmp/project/.agentbox/nix/store:/nix/store".to_owned()));
    assert!(!args.contains(&"/tmp/project/.agentbox/nix/var/nix:/nix/var/nix".to_owned()));
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
        "/tmp/project/.agentbox/nix-merged:/nix",
    );

    assert_eq!(args[0], "run");
    assert!(args.contains(&"-d".to_owned()));
    assert!(!args.contains(&"--rm".to_owned()));
    assert!(args.contains(&"--name".to_owned()));
    assert!(args.contains(&"agentbox-nix-sidecar-abc".to_owned()));
    assert!(args.contains(&"--user".to_owned()));
    assert!(args.contains(&"0:0".to_owned()));
    assert!(args.contains(&"--volume".to_owned()));
    assert!(args.contains(&"/tmp/project/.agentbox/nix-merged:/nix".to_owned()));
    assert_eq!(args[args.len() - 3], "bash");
    assert_eq!(args[args.len() - 2], "-lc");
    assert!(args[args.len() - 1].contains("nix-daemon --daemon"));
}

#[test]
fn build_socket_ping_podman_args_targets_nix_remote_socket() {
    let args =
        build_socket_ping_podman_args(DEFAULT_IMAGE, "/tmp/project/.agentbox/nix-merged:/nix:ro");

    assert!(args.contains(&"--userns".to_owned()));
    assert!(args.contains(&"keep-id".to_owned()));
    assert!(args.contains(&"/tmp/project/.agentbox/nix-merged:/nix:ro".to_owned()));
    assert_eq!(
        args[args.len() - 1],
        format!("nix store ping --store {NIX_REMOTE_SOCKET}")
    );
}

#[test]
fn sidecar_socket_timeout_error_includes_auto_cleanup_and_log_tail() {
    let merged_dir = Path::new("/tmp/project/.agentbox/nix-merged");
    let cleanup_outcome = SidecarStartupCleanupOutcome {
        summary: "removed sidecar 'agentbox-nix-sidecar-abc' (or it was already absent); cleaned merged mount '/tmp/project/.agentbox/nix-merged'".to_owned(),
        manual_merged_cleanup_required: false,
    };
    let diagnostics = SidecarStartupDiagnostics {
        sidecar_logs: Some("daemon booting\nready".to_owned()),
        sidecar_logs_error: None,
        socket_probe_failure: Some(
            "probe exited with status 1; stderr: permission denied".to_owned(),
        ),
        sidecar_state: Some(
            "running=true status=running exit_code=0 error= oom_killed=false".to_owned(),
        ),
        host_socket_exists: Some(false),
    };
    let message = build_sidecar_socket_timeout_error(
        "agentbox-nix-sidecar-abc",
        merged_dir,
        &cleanup_outcome,
        &diagnostics,
    );

    assert!(message.contains("was not connectable after startup"));
    assert!(message.contains("retrying should not require manual '.agentbox/nix-merged' removal"));
    assert!(message.contains("recent sidecar logs:\ndaemon booting\nready"));
    assert!(message.contains("socket probe failure: probe exited with status 1"));
    assert!(message.contains("sidecar state: running=true status=running exit_code=0"));
    assert!(message.contains("host socket path exists: no"));
    assert!(!message.contains("remove it before retrying"));
}

#[test]
fn sidecar_socket_timeout_error_requests_manual_cleanup_when_auto_cleanup_fails() {
    let merged_dir = Path::new("/tmp/project/.agentbox/nix-merged");
    let cleanup_outcome = SidecarStartupCleanupOutcome {
        summary: "failed to clean merged mount '/tmp/project/.agentbox/nix-merged': boom"
            .to_owned(),
        manual_merged_cleanup_required: true,
    };
    let diagnostics = SidecarStartupDiagnostics {
        sidecar_logs: None,
        sidecar_logs_error: Some(
            "failed to read nix-daemon sidecar logs: no container with name".to_owned(),
        ),
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

    assert!(message.contains("could not be cleaned automatically; remove it before retrying"));
    assert!(message.contains(
        "sidecar logs unavailable (failed to read nix-daemon sidecar logs: no container with name)"
    ));
    assert!(message.contains("usually means the sidecar terminated before logs could be collected"));
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
    let paths = SidecarPaths::new(dir.path());
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
    let paths = SidecarPaths::new(dir.path());
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
    let paths = SidecarPaths::new(dir.path());
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
