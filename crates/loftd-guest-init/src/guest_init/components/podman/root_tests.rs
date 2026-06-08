use crate::guest_init::components::env::ContainerStoreBackend;
use crate::guest_init::components::podman::root::{PodmanPrepOperation, planned_operations};

#[test]
fn podman_prep_operation_order_keeps_root_setup_before_configs() {
    let ops = planned_operations();
    let pos = |op| ops.iter().position(|candidate| candidate == &op).unwrap();
    assert_eq!(pos(PodmanPrepOperation::WriteRunningStatus), 0);
    assert!(pos(PodmanPrepOperation::PrepareTun) < pos(PodmanPrepOperation::PrepareKvm));
    assert!(pos(PodmanPrepOperation::PrepareKvm) < pos(PodmanPrepOperation::MaterializeSubids));
    assert!(
        pos(PodmanPrepOperation::MaterializeSubids) < pos(PodmanPrepOperation::InstallIdmapHelpers)
    );
    assert!(
        pos(PodmanPrepOperation::MountContainerStorage) < pos(PodmanPrepOperation::WriteConfig)
    );
    assert!(pos(PodmanPrepOperation::WriteConfig) < pos(PodmanPrepOperation::WriteReadyStatus));
}

#[test]
fn podman_prep_tools_are_backend_specific() {
    assert_eq!(
        super::required_tools(ContainerStoreBackend::Bind),
        ["podman"]
    );
    assert_eq!(
        super::required_tools(ContainerStoreBackend::RawDisk),
        ["blkid", "mount", "findmnt", "podman"]
    );
}
