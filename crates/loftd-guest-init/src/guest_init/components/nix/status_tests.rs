use crate::guest_init::components::nix::status::{
    NixPrepState, NixPrepStatus, format_wait_timeout, mark_ready_for_pid, read_status,
    write_running_unless_terminal, write_status,
};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tempfile::tempdir;

#[test]
fn nix_status_parses_missing_as_not_started() {
    let temp = tempdir().unwrap();
    let status = read_status(&temp.path().join("missing.status")).unwrap();
    assert_eq!(status.state, NixPrepState::NotStarted);
}

#[test]
fn nix_status_allows_legal_transitions() {
    let running = NixPrepStatus::running(42, PathBuf::from("/run/loftd/nix.log"));
    NixPrepStatus::not_started()
        .ensure_transition(NixPrepState::Running)
        .unwrap();
    assert_eq!(
        NixPrepStatus::ready_from(&running).unwrap().state,
        NixPrepState::Ready
    );
    assert_eq!(
        NixPrepStatus::failed_from(&running, "boom").unwrap().state,
        NixPrepState::Failed
    );
}

#[test]
fn nix_status_rejects_ready_to_running() {
    let ready = NixPrepStatus {
        state: NixPrepState::Ready,
        pid: Some(1),
        started_at: Some(1),
        finished_at: Some(2),
        log_path: Some(PathBuf::from("/log")),
        error: None,
    };
    let err = ready.ensure_transition(NixPrepState::Running).unwrap_err();
    assert!(
        err.to_string()
            .contains("illegal nix prep status transition")
    );
}

#[test]
fn nix_status_round_trips_failed_with_log_path() {
    let running = NixPrepStatus::running(7, PathBuf::from("/run/loftd/nix-prep.log"));
    let failed = NixPrepStatus::failed_from(&running, "line one\nline two").unwrap();
    let parsed = NixPrepStatus::from_text(&failed.to_text()).unwrap();
    assert_eq!(parsed.state, NixPrepState::Failed);
    assert_eq!(parsed.pid, Some(7));
    assert_eq!(
        parsed.log_path,
        Some(PathBuf::from("/run/loftd/nix-prep.log"))
    );
    assert_eq!(parsed.error.as_deref(), Some("line one\nline two"));
}
#[test]
fn nix_status_running_write_does_not_overwrite_terminal_state() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("nix-prep.status");
    let running = NixPrepStatus::running(7, PathBuf::from("/run/loftd/nix-prep.log"));
    let ready = NixPrepStatus::ready_from(&running).unwrap();
    write_status(&path, &ready).unwrap();

    let stale_running = NixPrepStatus::running(8, PathBuf::from("/run/loftd/nix-prep.log"));
    let wrote = write_running_unless_terminal(&path, &stale_running).unwrap();

    assert!(!wrote);
    let current = read_status(&path).unwrap();
    assert_eq!(current.state, NixPrepState::Ready);
    assert_eq!(current.pid, Some(7));
}

#[test]
fn nix_status_terminal_writes_require_current_running_pid() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("nix-prep.status");
    let running = NixPrepStatus::running(7, PathBuf::from("/run/loftd/nix-prep.log"));
    write_status(&path, &running).unwrap();

    let err = mark_ready_for_pid(&path, 8).unwrap_err();

    assert!(err.to_string().contains("no longer running for pid 8"));
    assert_eq!(read_status(&path).unwrap().state, NixPrepState::Running);
}

#[test]
fn nix_wait_timeout_message_includes_status_and_log() {
    let status = NixPrepStatus::running(99, PathBuf::from("/run/loftd/nix-prep.log"));
    let message = format_wait_timeout(
        Path::new("/run/loftd/nix-prep.status"),
        &status,
        Duration::from_secs(120),
    );
    assert!(message.contains("timed out after 120s"));
    assert!(message.contains("/run/loftd/nix-prep.status"));
    assert!(message.contains("/run/loftd/nix-prep.log"));
}
