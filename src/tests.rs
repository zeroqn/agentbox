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
fn task_kvm_mode_requires_sidecar_nix_runtime() {
    let err = validate_task_mode(TaskContainerMode::KvmKrunExperimental, false)
        .expect_err("task KVM without sidecar should fail");

    assert!(err
        .to_string()
        .contains("--task-kvm requires native nix sidecar mode"));
}

#[test]
fn task_kvm_mode_accepts_sidecar_nix_runtime() {
    validate_task_mode(TaskContainerMode::KvmKrunExperimental, true)
        .expect("task KVM with sidecar should be valid");
}

#[test]
fn native_task_mode_accepts_seeded_nix_runtime() {
    validate_task_mode(TaskContainerMode::Native, false)
        .expect("native task with seeded nix runtime should be valid");
}
