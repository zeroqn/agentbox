use super::*;

#[test]
fn task_hostname_uses_current_directory_name() {
    assert_eq!(
        derive_task_hostname(std::path::Path::new("/tmp/project")),
        "project-agentbox"
    );
}

#[test]
fn task_hostname_sanitizes_current_directory_name() {
    assert_eq!(
        derive_task_hostname(std::path::Path::new("/tmp/My repo.name!")),
        "my-repo-name-agentbox"
    );
}

#[test]
fn task_hostname_falls_back_when_directory_name_has_no_slug_chars() {
    assert_eq!(
        derive_task_hostname(std::path::Path::new("/tmp/!!!")),
        "workspace-agentbox"
    );
}

#[test]
fn sidecar_only_mode_requires_sidecar_nix_runtime() {
    let err = validate_sidecar_only_mode(true, false)
        .expect_err("sidecar-only without sidecar should fail");

    let message = err.to_string();
    assert!(message.contains("--sidecar-only requires nix sidecar mode"));
    assert!(message.contains("--disable-nix-sidecar"));
    assert!(message.contains("AGENTBOX_NIX_SIDECAR=0"));
}

#[test]
fn sidecar_only_mode_accepts_sidecar_nix_runtime() {
    validate_sidecar_only_mode(true, true).expect("sidecar-only with sidecar should be valid");
}

#[test]
fn sidecar_only_validation_precedes_generic_libkrun_sidecar_validation() {
    let err = validate_sidecar_only_mode(true, false)
        .and_then(|_| validate_runtime_mode(ContainerRuntimeMode::Libkrun, false, false))
        .expect_err("sidecar-only disabled sidecar should fail with sidecar-only error first");

    let message = err.to_string();
    assert!(message.contains("--sidecar-only requires nix sidecar mode"));
    assert!(!message.contains("default libkrun runtime requires nix sidecar mode"));
}

#[test]
fn sidecar_only_branch_skips_task_launch_and_idle_cleanup() {
    assert!(!should_launch_task_container(true));
    assert!(!should_cleanup_idle_sidecar_after_run(true));
    assert!(should_launch_task_container(false));
    assert!(should_cleanup_idle_sidecar_after_run(false));
}

#[test]
fn sidecar_only_branch_disables_socket_health_probe() {
    assert_eq!(
        sidecar_socket_health_probe(true),
        SidecarSocketHealthProbe::Disabled
    );
    assert_eq!(
        sidecar_socket_health_probe(false),
        SidecarSocketHealthProbe::Enabled
    );
}

#[test]
fn default_libkrun_mode_requires_sidecar_nix_runtime() {
    let err = validate_runtime_mode(ContainerRuntimeMode::Libkrun, false, false)
        .expect_err("libkrun without sidecar should fail");

    let message = err.to_string();
    assert!(message.contains("default libkrun runtime requires nix sidecar mode"));
    assert!(message.contains("--native"));
}

#[test]
fn default_libkrun_mode_accepts_sidecar_nix_runtime() {
    validate_runtime_mode(ContainerRuntimeMode::Libkrun, true, false)
        .expect("libkrun with sidecar should be valid");
}

#[test]
fn native_runtime_mode_accepts_seeded_nix_runtime() {
    validate_runtime_mode(ContainerRuntimeMode::Native, false, false)
        .expect("native task with seeded nix runtime should be valid");
}

#[test]
fn resolve_container_runtime_mode_defaults_to_libkrun() {
    assert_eq!(
        resolve_container_runtime_mode(false),
        ContainerRuntimeMode::Libkrun
    );
}

#[test]
fn resolve_container_runtime_mode_accepts_native_opt_out() {
    assert_eq!(
        resolve_container_runtime_mode(true),
        ContainerRuntimeMode::Native
    );
}

#[test]
fn native_runtime_mode_rejects_explicit_mem() {
    let err = validate_runtime_mode(ContainerRuntimeMode::Native, true, true)
        .expect_err("native task with --mem should fail");
    assert!(err.to_string().contains("--mem is only supported"));
}
