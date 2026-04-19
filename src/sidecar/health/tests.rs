use super::*;

#[test]
fn build_socket_ping_podman_args_targets_nix_remote_socket() {
    let args = build_socket_ping_podman_args(
        crate::DEFAULT_IMAGE,
        "/tmp/state/agentbox/project/nix-merged:/nix:ro",
    );

    assert!(args.contains(&"--userns".to_owned()));
    assert!(args.contains(&"keep-id".to_owned()));
    assert!(args.contains(&"/tmp/state/agentbox/project/nix-merged:/nix:ro".to_owned()));
    assert_eq!(
        args[args.len() - 1],
        format!("nix store ping --store {}", crate::NIX_REMOTE_SOCKET)
    );
}

#[test]
fn sidecar_socket_timeout_error_includes_auto_cleanup_and_log_tail() {
    let merged_dir = std::path::Path::new("/tmp/state/agentbox/project/nix-merged");
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
    let merged_dir = std::path::Path::new("/tmp/state/agentbox/project/nix-merged");
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
