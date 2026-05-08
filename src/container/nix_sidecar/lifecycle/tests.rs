use super::*;

#[test]
fn idle_sidecar_cleanup_is_preserved_while_task_containers_run() {
    assert!(preserve_idle_sidecar(true));
}

#[test]
fn idle_sidecar_cleanup_is_allowed_when_no_task_containers_run() {
    assert!(!preserve_idle_sidecar(false));
}
