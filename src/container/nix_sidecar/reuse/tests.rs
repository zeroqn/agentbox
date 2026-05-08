use super::*;
use std::path::PathBuf;

#[test]
fn protected_same_repo_reuse_requires_match_running_sidecar_and_running_task() {
    assert!(protected_same_repo_reuse_applies(true, true, Ok(true)));
}

#[test]
fn protected_same_repo_reuse_rejects_missing_matching_task_container() {
    assert!(!protected_same_repo_reuse_applies(true, true, Ok(false)));
}

#[test]
fn protected_same_repo_reuse_rejects_missing_running_sidecar() {
    assert!(!protected_same_repo_reuse_applies(true, false, Ok(true)));
}

#[test]
fn protected_same_repo_reuse_rejects_identity_mismatch() {
    assert!(!protected_same_repo_reuse_applies(false, true, Ok(true)));
}

#[test]
fn protected_same_repo_reuse_falls_back_when_task_probe_errors() {
    assert!(!protected_same_repo_reuse_applies(
        true,
        true,
        Err(anyhow::anyhow!("podman ps failed")),
    ));
}

#[test]
fn fallback_health_gated_reuse_keeps_existing_behavior_when_probe_fails() {
    assert!(fallback_health_gated_reuse_applies(true, false, true));
}

#[test]
fn fallback_health_gated_reuse_rebuilds_when_health_check_fails() {
    assert!(!fallback_health_gated_reuse_applies(true, false, false));
}

#[test]
fn fallback_health_gated_reuse_is_skipped_after_protected_reuse() {
    assert!(!fallback_health_gated_reuse_applies(true, true, true));
}

#[test]
fn legacy_non_native_sidecar_state_is_not_reused_as_container_native() {
    let state = SidecarState {
        image: crate::DEFAULT_IMAGE.to_owned(),
        image_id: "sha256:abc123".to_owned(),
        image_mount_path: PathBuf::from("/tmp/podman/mounts/abc"),
        sidecar_name: "agentbox-nix-sidecar-abc".to_owned(),
        mount_mode: crate::container::nix_sidecar::types::PodmanImageMountMode::Direct,
        proxy_port: Some(12345),
        native_config: false,
    };

    assert!(!state.matches(
        crate::DEFAULT_IMAGE,
        "sha256:abc123",
        "agentbox-nix-sidecar-abc"
    ));
    assert!(active_legacy_sidecar_config_applies(
        &state,
        crate::DEFAULT_IMAGE,
        "sha256:abc123",
        "agentbox-nix-sidecar-abc",
        true
    ));
    assert!(!active_legacy_sidecar_config_applies(
        &state,
        crate::DEFAULT_IMAGE,
        "sha256:abc123",
        "agentbox-nix-sidecar-abc",
        false
    ));
}
