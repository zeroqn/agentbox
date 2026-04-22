use super::*;
use std::fs;
use std::path::{Path, PathBuf};

#[test]
fn sidecar_paths_use_generation_scoped_layout() {
    let paths = SidecarPaths::new(Path::new("/tmp/state/agentbox/project"));
    assert_eq!(
        paths.sidecar_root,
        Path::new("/tmp/state/agentbox/project/nix-sidecar")
    );
    assert_eq!(
        paths.generations_dir,
        Path::new("/tmp/state/agentbox/project/nix-sidecar/generations")
    );
    assert_eq!(
        paths.current_pointer,
        Path::new("/tmp/state/agentbox/project/nix-sidecar/current")
    );
    assert_eq!(
        paths.lock_dir,
        Path::new("/tmp/state/agentbox/project/nix-sidecar/lock")
    );
    assert_eq!(
        paths.legacy_state_file,
        Path::new("/tmp/state/agentbox/project/nix-sidecar.state")
    );
}

#[test]
fn sidecar_name_depends_on_generation() {
    let cwd = Path::new("/tmp/project");
    let image_id = "sha256:abc123";
    let first = name::derive_sidecar_name(cwd, image_id, "gen-a");
    let second = name::derive_sidecar_name(cwd, image_id, "gen-a");
    let third = name::derive_sidecar_name(cwd, image_id, "gen-b");

    assert_eq!(first, second);
    assert_ne!(first, third);
    assert!(first.starts_with("agentbox-nix-sidecar-project-"));
}

#[test]
fn legacy_generation_id_prefixes_slugged_sidecar_name() {
    let generation = name::derive_legacy_generation_id("agentbox-nix-sidecar-abc");
    assert!(generation.starts_with("legacy-agentbox-nix-sidecar-abc"));
}

#[test]
fn build_sidecar_task_probe_args_filters_for_generation() {
    let args = build_sidecar_task_probe_args("gen-abc");
    assert_eq!(
        args,
        vec![
            "ps".to_owned(),
            "--filter".to_owned(),
            format!("label={TASK_CONTAINER_ROLE_LABEL}={TASK_CONTAINER_ROLE_VALUE}"),
            "--filter".to_owned(),
            format!("label={TASK_CONTAINER_GENERATION_LABEL}=gen-abc"),
            "--format".to_owned(),
            "{{.ID}}".to_owned(),
        ]
    );
}

#[test]
fn build_sidecar_podman_args_runs_daemon_as_root_and_mounts_rw_nix() {
    let args = build_sidecar_podman_args(
        crate::DEFAULT_IMAGE,
        "agentbox-nix-sidecar-abc",
        "/tmp/state/agentbox/project/nix-sidecar/generations/gen-abc/merged:/nix",
    );

    assert_eq!(args[0], "run");
    assert!(args.contains(&"-d".to_owned()));
    assert!(!args.contains(&"--rm".to_owned()));
    assert!(args.contains(&"--name".to_owned()));
    assert!(args.contains(&"agentbox-nix-sidecar-abc".to_owned()));
    assert!(args.contains(&"--user".to_owned()));
    assert!(args.contains(&"0:0".to_owned()));
    assert!(args.contains(&"--volume".to_owned()));
    assert!(args.contains(
        &"/tmp/state/agentbox/project/nix-sidecar/generations/gen-abc/merged:/nix".to_owned()
    ));
    assert_eq!(args[args.len() - 3], "bash");
    assert_eq!(args[args.len() - 2], "-lc");
    assert!(args[args.len() - 1].contains("rm -f /nix/var/nix/daemon-socket/socket"));
}

#[test]
fn generation_state_round_trip_via_record_file() {
    let dir = tempfile::tempdir().expect("tempdir should be created");
    let paths = SidecarPaths::new(&dir.path().join("state").join("agentbox").join("project"));
    let state = SidecarState {
        generation: "gen-abc".to_owned(),
        image: crate::DEFAULT_IMAGE.to_owned(),
        image_id: "sha256:abc123".to_owned(),
        image_mount_path: PathBuf::from("/tmp/podman/mounts/abc"),
        sidecar_name: "agentbox-nix-sidecar-abc".to_owned(),
        mount_mode: PodmanImageMountMode::Unshare,
        merged_dir: PathBuf::from(
            "/tmp/state/agentbox/project/nix-sidecar/generations/gen-abc/merged",
        ),
        upper_dir: PathBuf::from(
            "/tmp/state/agentbox/project/nix-sidecar/generations/gen-abc/upper",
        ),
        work_dir: PathBuf::from("/tmp/state/agentbox/project/nix-sidecar/generations/gen-abc/work"),
    };

    state::write_generation_record(&paths, &state).expect("record should be written");
    let parsed = state::read_generation_record(&paths, "gen-abc")
        .expect("record should parse")
        .expect("record should exist");

    assert_eq!(parsed, state);
}

#[test]
fn current_generation_round_trip_via_pointer_file() {
    let dir = tempfile::tempdir().expect("tempdir should be created");
    let paths = SidecarPaths::new(&dir.path().join("state").join("agentbox").join("project"));

    state::write_current_generation(&paths, "gen-abc").expect("current pointer should be written");
    let current = state::read_current_generation(&paths).expect("current pointer should parse");

    assert_eq!(current.as_deref(), Some("gen-abc"));
}

#[test]
fn legacy_state_migrates_into_generation_record() {
    let dir = tempfile::tempdir().expect("tempdir should be created");
    let state_root = dir.path().join("state").join("agentbox").join("project");
    let paths = SidecarPaths::new(&state_root);
    fs::create_dir_all(state_root.as_path()).expect("state root should exist");
    fs::write(
        &paths.legacy_state_file,
        "image=localhost/agentbox:latest\nimage_id=sha256:abc\nimage_mount_path=/tmp/podman/mount\nsidecar_name=agentbox-nix-sidecar-abc\nmount_mode=direct\n",
    )
    .expect("legacy state should be written");

    state::migrate_legacy_state_if_needed(&paths).expect("legacy migration should succeed");

    let current = state::read_current_generation(&paths)
        .expect("current pointer should parse")
        .expect("current pointer should exist");
    let parsed = state::read_generation_record(&paths, &current)
        .expect("generation record should parse")
        .expect("generation record should exist");

    assert_eq!(parsed.image, crate::DEFAULT_IMAGE);
    assert_eq!(parsed.image_id, "sha256:abc");
    assert_eq!(parsed.image_mount_path, PathBuf::from("/tmp/podman/mount"));
    assert_eq!(parsed.merged_dir, paths.legacy_merged_dir());
    assert_eq!(parsed.upper_dir, paths.legacy_upper_dir());
    assert_eq!(parsed.work_dir, paths.legacy_work_dir());
    assert!(paths.migrated_legacy_state_file().exists());
    assert!(!paths.legacy_state_file.exists());
}

#[test]
fn malformed_generation_record_is_ignored_without_deleting_siblings() {
    let dir = tempfile::tempdir().expect("tempdir should be created");
    let paths = SidecarPaths::new(&dir.path().join("state").join("agentbox").join("project"));
    fs::create_dir_all(paths.generation_root_dir("good")).expect("good dir should exist");
    fs::create_dir_all(paths.generation_root_dir("bad")).expect("bad dir should exist");
    fs::write(
        paths.generation_record_file("good"),
        "generation=good\nimage=localhost/agentbox:latest\nimage_id=sha256:abc\nimage_mount_path=/tmp/a\nsidecar_name=good\nmount_mode=direct\nmerged_dir=/tmp/good/merged\nupper_dir=/tmp/good/upper\nwork_dir=/tmp/good/work\n",
    )
    .expect("good record should be written");
    fs::write(paths.generation_record_file("bad"), "generation=bad\n")
        .expect("bad record should be written");

    let records = state::list_generation_records(&paths).expect("records should list");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].generation, "good");
    assert!(paths.generation_record_file("bad").exists());
}

#[test]
fn candidate_adoption_rejects_unhealthy_candidate_without_current() {
    assert_eq!(
        decide_candidate_adoption(false, false),
        CandidateAdoptionDecision::RejectCandidate
    );
}

#[test]
fn candidate_adoption_prefers_competing_current_over_candidate() {
    assert_eq!(
        decide_candidate_adoption(true, true),
        CandidateAdoptionDecision::UseCurrent
    );
    assert_eq!(
        decide_candidate_adoption(false, true),
        CandidateAdoptionDecision::UseCandidate
    );
}

#[test]
fn post_publish_prune_failures_are_non_fatal() {
    finish_post_publish_prune(Err(anyhow::anyhow!("boom")));
}
