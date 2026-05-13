use crate::guest_init::status;
use crate::guest_init::status::PodmanPrepStatus;
use crate::guest_init::user::podman::wait_for_status;
use std::path::PathBuf;
use std::time::Duration;
use tempfile::tempdir;

#[test]
fn podman_wait_returns_success_when_status_ready() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("status");
    let running = PodmanPrepStatus::running(1, PathBuf::from("/log"));
    let ready = PodmanPrepStatus::ready_from(&running).unwrap();
    status::write_status(&path, &ready).unwrap();
    wait_for_status(&path, Duration::from_millis(1), |_| false).unwrap();
}

#[test]
fn podman_wait_reports_failed_status() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("status");
    let running = PodmanPrepStatus::running(1, PathBuf::from("/run/agentbox/podman-prep.log"));
    let failed = PodmanPrepStatus::failed_from(&running, "boom").unwrap();
    status::write_status(&path, &failed).unwrap();
    let err = wait_for_status(&path, Duration::from_millis(1), |_| false).unwrap_err();
    assert!(err.to_string().contains("boom"));
    assert!(err.to_string().contains("/run/agentbox/podman-prep.log"));
}

#[test]
fn podman_wait_reports_stale_running_pid() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("status");
    let running = PodmanPrepStatus::running(9_999_999, PathBuf::from("/log"));
    status::write_status(&path, &running).unwrap();
    let err = wait_for_status(&path, Duration::from_millis(1), |_| false).unwrap_err();
    assert!(err.to_string().contains("stale/dead PID"));
}

#[test]
fn podman_wait_times_out_with_clear_message() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("status");
    let running = PodmanPrepStatus::running(123, PathBuf::from("/log"));
    status::write_status(&path, &running).unwrap();
    let err = wait_for_status(&path, Duration::from_millis(0), |_| true).unwrap_err();
    assert!(err.to_string().contains("timed out"));
    assert!(err.to_string().contains(path.to_str().unwrap()));
}
