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
fn default_libkrun_mode_requires_sidecar_nix_runtime() {
    let err = validate_task_mode(TaskContainerMode::Libkrun, false, false)
        .expect_err("libkrun without sidecar should fail");

    let message = err.to_string();
    assert!(message.contains("default libkrun task runtime requires native nix sidecar mode"));
    assert!(message.contains("--task-native"));
}

#[test]
fn default_libkrun_mode_accepts_sidecar_nix_runtime() {
    validate_task_mode(TaskContainerMode::Libkrun, true, false)
        .expect("libkrun with sidecar should be valid");
}

#[test]
fn native_task_mode_accepts_seeded_nix_runtime() {
    validate_task_mode(TaskContainerMode::Native, false, false)
        .expect("native task with seeded nix runtime should be valid");
}

#[test]
fn resolve_task_mode_defaults_to_libkrun() {
    assert_eq!(resolve_task_mode(false), TaskContainerMode::Libkrun);
}

#[test]
fn resolve_task_mode_accepts_native_opt_out() {
    assert_eq!(resolve_task_mode(true), TaskContainerMode::Native);
}

#[test]
fn native_task_mode_rejects_explicit_mem() {
    let err = validate_task_mode(TaskContainerMode::Native, true, true)
        .expect_err("native task with --mem should fail");
    assert!(err.to_string().contains("--mem is only supported"));
}
