use crate::guest_init::root::home::DevIdentity;
use crate::guest_init::root::podman::{
    containers_conf, planned_operations, policy_json, registries_conf, storage_conf,
    PodmanPrepOperation, PodmanToolPaths,
};
use std::path::PathBuf;

#[test]
fn podman_prep_operation_order_keeps_root_setup_before_configs() {
    let ops = planned_operations();
    let pos = |op| ops.iter().position(|candidate| candidate == &op).unwrap();
    assert_eq!(pos(PodmanPrepOperation::WriteRunningStatus), 0);
    assert!(pos(PodmanPrepOperation::PrepareTun) < pos(PodmanPrepOperation::MaterializeSubids));
    assert!(
        pos(PodmanPrepOperation::MaterializeSubids) < pos(PodmanPrepOperation::InstallIdmapHelpers)
    );
    assert!(
        pos(PodmanPrepOperation::MountContainerStorage) < pos(PodmanPrepOperation::WriteConfig)
    );
}

#[test]
fn podman_prep_generates_btrfs_storage_config_without_fallbacks() {
    let identity = DevIdentity::new(1000, 1000, PathBuf::from("fish"));
    let conf = storage_conf(&identity);
    assert!(conf.contains("driver = \"btrfs\""));
    assert!(conf.contains("graphroot = \"/home/dev/.local/share/containers/storage\""));
    assert!(conf.contains("runroot = \"/run/user/1000/containers\""));
    assert!(!conf.contains("fuse-overlayfs"));
    assert!(!conf.contains("driver = \"overlay\""));
    assert!(!conf.contains("driver = \"vfs\""));
    assert!(!conf.contains("mount_program"));
}

#[test]
fn podman_prep_generates_containers_config_for_rootless_libkrun() {
    let conf = containers_conf(&PodmanToolPaths::fixture());
    for required in [
        "cgroups = \"disabled\"",
        "cgroup_manager = \"cgroupfs\"",
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
