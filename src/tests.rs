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
