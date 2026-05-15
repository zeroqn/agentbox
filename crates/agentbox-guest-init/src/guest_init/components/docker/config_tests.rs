use std::path::PathBuf;

use crate::guest_init::components::docker::config::{DockerPaths, daemon_json};
use crate::guest_init::components::home::identity::DevIdentity;

#[test]
fn docker_paths_keep_persistent_state_on_containers_disk() {
    let paths = DockerPaths::for_identity(&DevIdentity::new(1000, 1000, PathBuf::from("fish")));

    assert_eq!(
        paths.data_root.to_str().unwrap(),
        "/home/dev/.local/share/containers/docker/data"
    );
    assert_eq!(
        paths.exec_root.to_str().unwrap(),
        "/home/dev/.local/share/containers/docker/exec"
    );
    assert_eq!(
        paths.state_root.to_str().unwrap(),
        "/home/dev/.local/share/containers/docker/state"
    );
    assert_eq!(paths.host_uri(), "unix:///run/user/1000/docker/docker.sock");
    assert_eq!(
        paths.daemon_status_path.to_str().unwrap(),
        "/run/user/1000/docker/daemon.status"
    );
    assert_eq!(
        paths.daemon_log_path.to_str().unwrap(),
        "/run/user/1000/docker/daemon.log"
    );
    assert_eq!(
        paths.daemon_start_lock_path.to_str().unwrap(),
        "/run/user/1000/docker/daemon-start.lock"
    );
}

#[test]
fn docker_daemon_config_forces_classic_btrfs_storage() {
    let paths = DockerPaths::for_identity(&DevIdentity::new(1000, 1000, PathBuf::from("fish")));
    let json = daemon_json(&paths);

    assert!(json.contains("\"storage-driver\": \"btrfs\""));
    assert!(json.contains("\"data-root\": \"/home/dev/.local/share/containers/docker/data\""));
    assert!(json.contains("\"exec-root\": \"/home/dev/.local/share/containers/docker/exec\""));
    assert!(json.contains("\"containerd-snapshotter\": false"));
    assert!(!json.contains("/var/lib/docker"));
    assert!(!json.contains("/var/lib/containerd"));
    assert!(!json.contains("/home/dev/.local/share/docker"));
    assert!(!json.contains("\"storage-driver\": \"overlay"));
    assert!(!json.contains("\"storage-driver\": \"vfs"));
}
