use crate::guest_init::components::docker::root::{DockerPrepOperation, planned_operations};

#[test]
fn docker_prep_operation_order_keeps_root_setup_before_daemon_config() {
    let ops = planned_operations();
    let pos = |op| ops.iter().position(|candidate| candidate == &op).unwrap();
    assert_eq!(pos(DockerPrepOperation::WriteRunningStatus), 0);
    assert!(pos(DockerPrepOperation::PrepareTun) < pos(DockerPrepOperation::MaterializeSubids));
    assert!(
        pos(DockerPrepOperation::MaterializeSubids) < pos(DockerPrepOperation::InstallIdmapHelpers)
    );
    assert!(
        pos(DockerPrepOperation::MountContainerStorage)
            < pos(DockerPrepOperation::WriteDaemonConfig)
    );
}

#[test]
fn docker_prep_owns_rootless_runtime_parent_without_podman() {
    let source = include_str!("root.rs");
    assert!(source.contains("rootless::runtime_dir::ensure_user_runtime_dir(identity)"));
}
