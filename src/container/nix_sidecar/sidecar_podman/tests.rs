use super::container::{build_sidecar_podman_args, SIDECAR_ENTRYPOINT};
use super::proxy::resolve_runtime_proxy_port_or_default;
use super::task_probe::build_sidecar_task_probe_args;
use crate::{TASK_CONTAINER_ROLE_LABEL, TASK_CONTAINER_ROLE_VALUE, TASK_CONTAINER_SIDECAR_LABEL};

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
        crate::DEFAULT_IMAGE,
        "agentbox-nix-sidecar-abc",
        "/tmp/state/agentbox/project/nix-merged:/nix",
    )
    .expect("sidecar args should build");

    assert_eq!(args[0], "run");
    assert!(args.contains(&"-d".to_owned()));
    assert!(!args.contains(&"--rm".to_owned()));
    assert!(args.contains(&"--name".to_owned()));
    assert!(args.contains(&"agentbox-nix-sidecar-abc".to_owned()));
    assert!(args.contains(&"--user".to_owned()));
    assert!(args.contains(&"0:0".to_owned()));
    assert!(args.contains(&"--volume".to_owned()));
    assert!(args.contains(&"/tmp/state/agentbox/project/nix-merged:/nix".to_owned()));
    assert!(args.contains(&"--publish".to_owned()));
    assert!(args.contains(&"19876".to_owned()));
    assert!(!args.contains(&"--runtime".to_owned()));
    assert!(!args.contains(&"run.oci.handler=krun".to_owned()));
    assert!(!args.iter().any(|arg| arg.contains("krun.")));
    assert!(!args.iter().any(|arg| arg.contains("all_proxy")));
    assert_uses_embedded_sidecar_entrypoint(&args);
}

#[test]
fn runtime_proxy_port_falls_back_to_legacy_default_on_resolution_error() {
    let port = resolve_runtime_proxy_port_or_default(Err(anyhow::anyhow!("podman port failed")));

    assert_eq!(port, 19876);
}

fn assert_uses_embedded_sidecar_entrypoint(args: &[String]) {
    assert!(args
        .windows(2)
        .any(|w| { w[0] == "--entrypoint" && w[1] == SIDECAR_ENTRYPOINT }));
    assert_eq!(args.last().map(String::as_str), Some(crate::DEFAULT_IMAGE));
    assert!(!args.windows(2).any(|w| w[0] == "bash" && w[1] == "-lc"));
    assert!(!args.iter().any(|arg| arg.contains("nix-daemon --daemon")));
}
