use super::*;

#[test]
fn build_proxy_socket_ping_podman_args_targets_local_socat_socket() {
    let args = build_proxy_socket_ping_podman_args(
        crate::DEFAULT_IMAGE,
        "host.containers.internal",
        45678,
    );

    assert!(args.contains(&"--userns".to_owned()));
    assert!(args.contains(&"keep-id".to_owned()));
    assert!(!args.contains(&"--volume".to_owned()));
    assert!(!args.contains(&"--runtime".to_owned()));
    assert!(!args.contains(&"run.oci.handler=krun".to_owned()));
    assert!(!args.iter().any(|arg| arg.contains("krun.")));

    let script = &args[args.len() - 1];
    assert!(script.contains("mktemp -u /tmp/agentbox-nix-health."));
    assert!(script.contains("socat \"UNIX-LISTEN:$probe_socket,fork,unlink-early,umask=000\""));
    assert!(script.contains("\"TCP:host.containers.internal:45678\""));
    assert!(script.contains("rm -f \"$probe_socket\""));
    assert!(script.contains("kill \"$socat_pid\""));
    assert!(script.contains("nix store ping --store \"unix://$probe_socket\""));
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
        proxy_port_listening: Some(false),
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
    assert!(message.contains("proxy port 19876 listening: no"));
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
        proxy_port_listening: None,
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
fn sidecar_socket_timeout_error_names_proxy_health_boundary() {
    let merged_dir = std::path::Path::new("/tmp/state/agentbox/project/nix-merged");
    let cleanup_outcome = SidecarStartupCleanupOutcome {
        summary: "removed sidecar 'agentbox-nix-sidecar-abc' (or it was already absent)".to_owned(),
        manual_merged_cleanup_required: false,
    };
    let diagnostics = SidecarStartupDiagnostics {
        socket_probe_failure: Some(
            "stderr: socat bridge did not create health probe socket".to_owned(),
        ),
        host_socket_exists: Some(true),
        proxy_port_listening: Some(true),
        ..SidecarStartupDiagnostics::default()
    };

    let message = build_sidecar_socket_timeout_error(
        "agentbox-nix-sidecar-abc",
        merged_dir,
        &cleanup_outcome,
        &diagnostics,
    );

    assert!(message.contains("nix-daemon proxy for socket"));
    assert!(message.contains("socket probe failure: stderr: socat bridge"));
    assert!(message.contains("host socket path exists: yes"));
    assert!(message.contains("proxy port 19876 listening: yes"));
}
