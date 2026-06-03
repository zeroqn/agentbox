use crate::guest_init::components::nix::status;
use crate::guest_init::components::nix::status::NixPrepStatus;
use crate::guest_init::components::nix::user::wait_for_status_and_socket;
use std::os::unix::fs::symlink;
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::time::Duration;
use tempfile::tempdir;

#[test]
fn nix_wait_returns_success_when_status_ready_and_socket_exists() {
    let temp = tempdir().unwrap();
    let status_path = temp.path().join("status");
    let socket_path = temp.path().join("socket");
    let _listener = UnixListener::bind(&socket_path).unwrap();
    let running = NixPrepStatus::running(1, PathBuf::from("/log"));
    let ready = NixPrepStatus::ready_from(&running).unwrap();
    status::write_status(&status_path, &ready).unwrap();

    wait_for_status_and_socket(&status_path, &socket_path, Duration::from_millis(1), |_| {
        false
    })
    .unwrap();
}

#[test]
fn nix_wait_reports_not_started_status() {
    let temp = tempdir().unwrap();
    let status_path = temp.path().join("status");
    let socket_path = temp.path().join("socket");

    let err =
        wait_for_status_and_socket(&status_path, &socket_path, Duration::from_millis(1), |_| {
            false
        })
        .unwrap_err();

    assert!(err.to_string().contains("has not started"));
    assert!(err.to_string().contains(status_path.to_str().unwrap()));
}

#[test]
fn nix_wait_reports_failed_status_with_log() {
    let temp = tempdir().unwrap();
    let status_path = temp.path().join("status");
    let socket_path = temp.path().join("socket");
    let running = NixPrepStatus::running(1, PathBuf::from("/run/loftd/nix-prep.log"));
    let failed = NixPrepStatus::failed_from(&running, "boom").unwrap();
    status::write_status(&status_path, &failed).unwrap();

    let err =
        wait_for_status_and_socket(&status_path, &socket_path, Duration::from_millis(1), |_| {
            false
        })
        .unwrap_err();

    assert!(err.to_string().contains("boom"));
    assert!(err.to_string().contains("/run/loftd/nix-prep.log"));
}

#[test]
fn nix_wait_reports_stale_running_pid() {
    let temp = tempdir().unwrap();
    let status_path = temp.path().join("status");
    let socket_path = temp.path().join("socket");
    let running = NixPrepStatus::running(9_999_999, PathBuf::from("/log"));
    status::write_status(&status_path, &running).unwrap();

    let err =
        wait_for_status_and_socket(&status_path, &socket_path, Duration::from_millis(1), |_| {
            false
        })
        .unwrap_err();

    assert!(err.to_string().contains("stale/dead PID"));
}

#[test]
fn nix_wait_times_out_while_running() {
    let temp = tempdir().unwrap();
    let status_path = temp.path().join("status");
    let socket_path = temp.path().join("socket");
    let running = NixPrepStatus::running(123, PathBuf::from("/log"));
    status::write_status(&status_path, &running).unwrap();

    let err =
        wait_for_status_and_socket(&status_path, &socket_path, Duration::from_millis(0), |_| {
            true
        })
        .unwrap_err();

    assert!(err.to_string().contains("timed out"));
    assert!(err.to_string().contains(status_path.to_str().unwrap()));
}

#[test]
fn nix_wait_reports_missing_socket_after_ready() {
    let temp = tempdir().unwrap();
    let status_path = temp.path().join("status");
    let socket_path = temp.path().join("socket");
    let running = NixPrepStatus::running(123, PathBuf::from("/log"));
    let ready = NixPrepStatus::ready_from(&running).unwrap();
    status::write_status(&status_path, &ready).unwrap();

    let err =
        wait_for_status_and_socket(&status_path, &socket_path, Duration::from_millis(0), |_| {
            true
        })
        .unwrap_err();

    assert!(err.to_string().contains("Unix socket"));
    assert!(err.to_string().contains("observed=missing"));
    assert!(err.to_string().contains(socket_path.to_str().unwrap()));
}

#[test]
fn nix_wait_rejects_non_socket_path_after_ready() {
    let temp = tempdir().unwrap();
    let status_path = temp.path().join("status");
    let socket_path = temp.path().join("socket");
    std::fs::write(&socket_path, "not a socket").unwrap();
    let running = NixPrepStatus::running(123, PathBuf::from("/log"));
    let ready = NixPrepStatus::ready_from(&running).unwrap();
    status::write_status(&status_path, &ready).unwrap();

    let err =
        wait_for_status_and_socket(&status_path, &socket_path, Duration::from_millis(0), |_| {
            true
        })
        .unwrap_err();

    assert!(err.to_string().contains("observed=regular-file"));
}

#[test]
fn nix_wait_rejects_symlink_to_socket_after_ready() {
    let temp = tempdir().unwrap();
    let status_path = temp.path().join("status");
    let real_socket_path = temp.path().join("real-socket");
    let socket_path = temp.path().join("socket-link");
    let _listener = UnixListener::bind(&real_socket_path).unwrap();
    symlink(&real_socket_path, &socket_path).unwrap();
    let running = NixPrepStatus::running(123, PathBuf::from("/log"));
    let ready = NixPrepStatus::ready_from(&running).unwrap();
    status::write_status(&status_path, &ready).unwrap();

    let err =
        wait_for_status_and_socket(&status_path, &socket_path, Duration::from_millis(0), |_| {
            true
        })
        .unwrap_err();

    assert!(err.to_string().contains("observed=symlink"));
}
