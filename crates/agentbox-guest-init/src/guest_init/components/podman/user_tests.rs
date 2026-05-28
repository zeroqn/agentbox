use crate::guest_init::components::podman::status;
use crate::guest_init::components::podman::status::PodmanPrepStatus;
use crate::guest_init::components::podman::user::{wait_for_status, wait_for_status_with_service};
use anyhow::anyhow;
use std::cell::Cell;
use std::path::PathBuf;
use std::time::Duration;
use tempfile::tempdir;

#[test]
fn podman_prep_wait_returns_success_when_status_ready() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("status");
    let running = PodmanPrepStatus::running(1, PathBuf::from("/log"));
    let ready = PodmanPrepStatus::ready_from(&running).unwrap();
    status::write_status(&path, &ready).unwrap();
    wait_for_status(&path, Duration::from_millis(1), |_| false).unwrap();
}

#[test]
fn podman_service_wait_runs_service_after_prep_ready() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("status");
    let running = PodmanPrepStatus::running(1, PathBuf::from("/log"));
    let ready = PodmanPrepStatus::ready_from(&running).unwrap();
    status::write_status(&path, &ready).unwrap();
    let calls = Cell::new(0);

    wait_for_status_with_service(
        &path,
        Duration::from_millis(1),
        |_| false,
        || {
            calls.set(calls.get() + 1);
            Ok(())
        },
    )
    .unwrap();

    assert_eq!(calls.get(), 1);
}

#[test]
fn podman_wait_reports_ready_status_when_socket_repair_fails() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("status");
    let running = PodmanPrepStatus::running(1, PathBuf::from("/log"));
    let ready = PodmanPrepStatus::ready_from(&running).unwrap();
    status::write_status(&path, &ready).unwrap();
    let err = wait_for_status_with_service(
        &path,
        Duration::from_millis(1),
        |_| false,
        || Err(anyhow!("socket missing")),
    )
    .unwrap_err();
    let message = format!("{err:#}");
    assert!(message.contains("ready but API socket is not live"));
    assert!(message.contains("socket missing"));
}

#[test]
fn podman_wait_reports_failed_status_without_masking_it_with_service_liveness() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("status");
    let running = PodmanPrepStatus::running(1, PathBuf::from("/run/agentbox/podman-prep.log"));
    let failed = PodmanPrepStatus::failed_from(&running, "boom").unwrap();
    status::write_status(&path, &failed).unwrap();
    let calls = Cell::new(0);

    let err = wait_for_status_with_service(
        &path,
        Duration::from_millis(1),
        |_| false,
        || {
            calls.set(calls.get() + 1);
            Ok(())
        },
    )
    .unwrap_err();

    let message = format!("{err:#}");
    assert!(message.contains("boom"));
    assert!(message.contains("/run/agentbox/podman-prep.log"));
    assert_eq!(calls.get(), 0);
}

#[test]
fn podman_wait_reports_stale_running_pid_without_masking_it_with_service_liveness() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("status");
    let running = PodmanPrepStatus::running(9_999_999, PathBuf::from("/log"));
    status::write_status(&path, &running).unwrap();
    let calls = Cell::new(0);

    let err = wait_for_status_with_service(
        &path,
        Duration::from_millis(1),
        |_| false,
        || {
            calls.set(calls.get() + 1);
            Ok(())
        },
    )
    .unwrap_err();

    assert!(err.to_string().contains("stale/dead PID"));
    assert_eq!(calls.get(), 0);
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
