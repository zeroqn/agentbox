use crate::guest_init::components::podman::status::{
    PodmanPrepState, PodmanPrepStatus, format_wait_timeout, mark_ready_for_pid, read_status,
    write_running_unless_terminal, write_status,
};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tempfile::tempdir;

#[test]
fn podman_status_parses_missing_as_not_started() {
    let temp = tempdir().unwrap();
    let status = read_status(&temp.path().join("missing.status")).unwrap();
    assert_eq!(status.state, PodmanPrepState::NotStarted);
}

#[test]
fn podman_status_allows_legal_transitions() {
    let running = PodmanPrepStatus::running(42, PathBuf::from("/run/loftd/podman.log"));
    PodmanPrepStatus::not_started()
        .ensure_transition(PodmanPrepState::Running)
        .unwrap();
    assert_eq!(
        PodmanPrepStatus::ready_from(&running).unwrap().state,
        PodmanPrepState::Ready
    );
    assert_eq!(
        PodmanPrepStatus::failed_from(&running, "boom")
            .unwrap()
            .state,
        PodmanPrepState::Failed
    );
}

#[test]
fn podman_status_rejects_ready_to_running() {
    let ready = PodmanPrepStatus {
        state: PodmanPrepState::Ready,
        pid: Some(1),
        started_at: Some(1),
        finished_at: Some(2),
        log_path: Some(PathBuf::from("/log")),
        error: None,
    };
    let err = ready
        .ensure_transition(PodmanPrepState::Running)
        .unwrap_err();
    assert!(
        err.to_string()
            .contains("illegal podman prep status transition")
    );
}

#[test]
fn podman_status_round_trips_failed_with_log_path() {
    let running = PodmanPrepStatus::running(7, PathBuf::from("/run/loftd/podman-prep.log"));
    let failed = PodmanPrepStatus::failed_from(&running, "line one\nline two").unwrap();
    let parsed = PodmanPrepStatus::from_text(&failed.to_text()).unwrap();
    assert_eq!(parsed.state, PodmanPrepState::Failed);
    assert_eq!(parsed.pid, Some(7));
    assert_eq!(
        parsed.log_path,
        Some(PathBuf::from("/run/loftd/podman-prep.log"))
    );
    assert_eq!(parsed.error.as_deref(), Some("line one\nline two"));
}
#[test]
fn podman_status_running_write_does_not_overwrite_terminal_state() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("podman-prep.status");
    let running = PodmanPrepStatus::running(7, PathBuf::from("/run/loftd/podman-prep.log"));
    let ready = PodmanPrepStatus::ready_from(&running).unwrap();
    write_status(&path, &ready).unwrap();

    let stale_running = PodmanPrepStatus::running(8, PathBuf::from("/run/loftd/podman-prep.log"));
    let wrote = write_running_unless_terminal(&path, &stale_running).unwrap();

    assert!(!wrote);
    let current = read_status(&path).unwrap();
    assert_eq!(current.state, PodmanPrepState::Ready);
    assert_eq!(current.pid, Some(7));
}

#[test]
fn podman_status_terminal_writes_require_current_running_pid() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("podman-prep.status");
    let running = PodmanPrepStatus::running(7, PathBuf::from("/run/loftd/podman-prep.log"));
    write_status(&path, &running).unwrap();

    let err = mark_ready_for_pid(&path, 8).unwrap_err();

    assert!(err.to_string().contains("no longer running for pid 8"));
    assert_eq!(read_status(&path).unwrap().state, PodmanPrepState::Running);
}

#[test]
fn podman_wait_timeout_message_includes_status_and_log() {
    let status = PodmanPrepStatus::running(99, PathBuf::from("/run/loftd/podman-prep.log"));
    let message = format_wait_timeout(
        Path::new("/run/loftd/podman-prep.status"),
        &status,
        Duration::from_secs(120),
    );
    assert!(message.contains("timed out after 120s"));
    assert!(message.contains("/run/loftd/podman-prep.status"));
    assert!(message.contains("/run/loftd/podman-prep.log"));
}
