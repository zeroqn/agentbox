use std::path::{Path, PathBuf};
use std::time::Duration;

use tempfile::tempdir;

use crate::guest_init::components::docker::status::{
    format_wait_timeout, mark_ready_for_pid, read_status, write_running_unless_terminal,
    write_status, DockerState, DockerStatus,
};

#[test]
fn docker_status_parses_missing_as_not_started() {
    let temp = tempdir().unwrap();
    let status = read_status(&temp.path().join("missing.status")).unwrap();
    assert_eq!(status.state, DockerState::NotStarted);
}

#[test]
fn docker_status_allows_legal_transitions() {
    let running = DockerStatus::running(42, PathBuf::from("/run/agentbox/docker.log"));
    DockerStatus::not_started()
        .ensure_transition(DockerState::Running)
        .unwrap();
    assert_eq!(
        DockerStatus::ready_from(&running).unwrap().state,
        DockerState::Ready
    );
    assert_eq!(
        DockerStatus::failed_from(&running, "boom").unwrap().state,
        DockerState::Failed
    );
}

#[test]
fn docker_status_round_trips_failed_with_log_path() {
    let running = DockerStatus::running(7, PathBuf::from("/run/user/1000/docker/daemon.log"));
    let failed = DockerStatus::failed_from(&running, "line one\nline two").unwrap();
    let parsed = DockerStatus::from_text(&failed.to_text()).unwrap();
    assert_eq!(parsed.state, DockerState::Failed);
    assert_eq!(parsed.pid, Some(7));
    assert_eq!(
        parsed.log_path,
        Some(PathBuf::from("/run/user/1000/docker/daemon.log"))
    );
    assert_eq!(parsed.error.as_deref(), Some("line one\nline two"));
}

#[test]
fn docker_status_running_write_does_not_overwrite_terminal_state() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("docker.status");
    let running = DockerStatus::running(7, PathBuf::from("/run/user/1000/docker/daemon.log"));
    let ready = DockerStatus::ready_from(&running).unwrap();
    write_status(&path, &ready).unwrap();

    let stale_running = DockerStatus::running(8, PathBuf::from("/run/user/1000/docker/daemon.log"));
    let wrote = write_running_unless_terminal(&path, &stale_running).unwrap();

    assert!(!wrote);
    let current = read_status(&path).unwrap();
    assert_eq!(current.state, DockerState::Ready);
    assert_eq!(current.pid, Some(7));
}

#[test]
fn docker_status_terminal_writes_require_current_running_pid() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("docker.status");
    let running = DockerStatus::running(7, PathBuf::from("/run/user/1000/docker/daemon.log"));
    write_status(&path, &running).unwrap();

    let err = mark_ready_for_pid(&path, 8).unwrap_err();

    assert!(err.to_string().contains("no longer running for pid 8"));
    assert_eq!(read_status(&path).unwrap().state, DockerState::Running);
}

#[test]
fn docker_wait_timeout_message_includes_label_status_and_log() {
    let status = DockerStatus::running(99, PathBuf::from("/run/user/1000/docker/daemon.log"));
    let message = format_wait_timeout(
        "daemon",
        Path::new("/run/user/1000/docker/daemon.status"),
        &status,
        Duration::from_secs(120),
    );
    assert!(message.contains("rootless Docker daemon"));
    assert!(message.contains("timed out after 120s"));
    assert!(message.contains("/run/user/1000/docker/daemon.status"));
    assert!(message.contains("/run/user/1000/docker/daemon.log"));
}
