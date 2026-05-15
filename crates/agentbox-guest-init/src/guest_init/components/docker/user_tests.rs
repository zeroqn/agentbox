use std::path::PathBuf;
use std::time::Duration;

use tempfile::tempdir;

use crate::guest_init::components::docker::config::DockerPaths;
use crate::guest_init::components::docker::status::{self, DockerStatus};
use crate::guest_init::components::docker::user::{validate_info_line, wait_for_status};
use crate::guest_init::components::home::identity::DevIdentity;

#[test]
fn docker_wait_returns_success_when_status_ready() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("status");
    let running = DockerStatus::running(1, PathBuf::from("/log"));
    let ready = DockerStatus::ready_from(&running).unwrap();
    status::write_status(&path, &ready).unwrap();
    wait_for_status("prep", &path, Duration::from_millis(1), |_| false).unwrap();
}

#[test]
fn docker_wait_reports_failed_status() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("status");
    let running = DockerStatus::running(1, PathBuf::from("/run/agentbox/docker-prep.log"));
    let failed = DockerStatus::failed_from(&running, "boom").unwrap();
    status::write_status(&path, &failed).unwrap();
    let err = wait_for_status("prep", &path, Duration::from_millis(1), |_| false).unwrap_err();
    assert!(err.to_string().contains("boom"));
    assert!(err.to_string().contains("/run/agentbox/docker-prep.log"));
}

#[test]
fn docker_wait_reports_stale_running_pid() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("status");
    let running = DockerStatus::running(9_999_999, PathBuf::from("/log"));
    status::write_status(&path, &running).unwrap();
    let err = wait_for_status("daemon", &path, Duration::from_millis(1), |_| false).unwrap_err();
    assert!(err.to_string().contains("stale/dead PID"));
}

#[test]
fn docker_info_validation_requires_btrfs_data_root_and_no_cgroups() {
    let paths = DockerPaths::for_identity(&DevIdentity::new(1000, 1000, PathBuf::from("fish")));
    validate_info_line(
        "btrfs|/home/dev/.local/share/containers/docker/data|none\n",
        &paths,
    )
    .unwrap();

    assert!(
        validate_info_line(
            "overlay2|/home/dev/.local/share/containers/docker/data|none\n",
            &paths,
        )
        .is_err()
    );
    assert!(validate_info_line("btrfs|/home/dev/.local/share/docker|none\n", &paths).is_err());
    assert!(
        validate_info_line(
            "btrfs|/home/dev/.local/share/containers/docker/data|systemd\n",
            &paths,
        )
        .is_err()
    );
}

#[test]
fn docker_info_probe_does_not_reenter_agentbox_docker_wrapper() {
    let source = include_str!("user.rs");
    assert!(source.contains("env_remove(\"AGENTBOX_LIBKRUN_CONTAINERS_STORAGE\")"));
}

#[test]
fn docker_daemon_live_running_status_waits_instead_of_starting_another() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("docker-daemon.status");
    let running = DockerStatus::running(std::process::id(), PathBuf::from("/log"));
    status::write_status(&path, &running).unwrap();

    assert!(crate::guest_init::components::docker::user::has_live_starting_daemon(&path).unwrap());
}

#[test]
fn docker_daemon_dead_running_status_is_not_treated_as_reusable() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("docker-daemon.status");
    let running = DockerStatus::running(9_999_999, PathBuf::from("/log"));
    status::write_status(&path, &running).unwrap();

    assert!(!crate::guest_init::components::docker::user::has_live_starting_daemon(&path).unwrap());
}

#[test]
fn docker_daemon_start_decision_uses_start_lock() {
    let source = include_str!("user.rs");
    assert!(source.contains("daemon_start_lock_path"));
    assert!(source.contains("DaemonStartDecision::WaitForExisting"));
}
