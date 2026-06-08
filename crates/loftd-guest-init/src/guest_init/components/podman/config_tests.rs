use crate::guest_init::components::env::ContainerStoreBackend;
use crate::guest_init::components::home::identity::DevIdentity;
use crate::guest_init::components::podman::config::{
    PodmanToolPaths, containers_conf, policy_json, registries_conf, storage_conf,
};
use std::path::PathBuf;

#[test]
fn podman_prep_generates_overlay_storage_config_for_bind_store() {
    let identity = DevIdentity::new(1000, 1000, PathBuf::from("fish"));
    let conf = storage_conf(&identity, ContainerStoreBackend::Bind);
    assert!(conf.contains("driver = \"overlay\""));
    assert!(conf.contains("graphroot = \"/home/dev/.local/share/containers/storage\""));
    assert!(conf.contains("runroot = \"/run/user/1000/containers\""));
    assert!(!conf.contains("fuse-overlayfs"));
    assert!(!conf.contains("driver = \"btrfs\""));
    assert!(!conf.contains("driver = \"vfs\""));
    assert!(!conf.contains("mount_program"));
}

#[test]
fn podman_prep_keeps_btrfs_storage_config_for_raw_disk_store() {
    let identity = DevIdentity::new(1000, 1000, PathBuf::from("fish"));
    let conf = storage_conf(&identity, ContainerStoreBackend::RawDisk);
    assert!(conf.contains("driver = \"btrfs\""));
    assert!(conf.contains("graphroot = \"/home/dev/.local/share/containers/storage\""));
    assert!(conf.contains("runroot = \"/run/user/1000/containers\""));
    assert!(!conf.contains("fuse-overlayfs"));
    assert!(!conf.contains("driver = \"overlay\""));
    assert!(!conf.contains("driver = \"vfs\""));
    assert!(!conf.contains("mount_program"));
}

#[test]
fn podman_prep_generates_containers_config_for_rootless_internal() {
    let conf = containers_conf(&PodmanToolPaths::fixture());
    for required in [
        "cgroups = \"disabled\"",
        "cgroup_manager = \"cgroupfs\"",
        "compose_warning_logs = false",
        "events_logger = \"file\"",
        "runtime = \"crun\"",
        "conmon_path = [\"/nix/store/conmon/bin/conmon\"]",
        "netavark",
        "aardvark-dns",
        "/nix/store/passt/bin",
        "crun = [\"/nix/store/crun/bin/crun\"]",
        "network_backend = \"netavark\"",
    ] {
        assert!(conf.contains(required), "missing {required}");
    }
}

#[test]
fn podman_prep_generates_registries_and_policy() {
    assert!(registries_conf().contains("registries = [\"docker.io\"]"));
    assert!(policy_json().contains("insecureAcceptAnything"));
    assert!(policy_json().contains("docker-daemon"));
}
