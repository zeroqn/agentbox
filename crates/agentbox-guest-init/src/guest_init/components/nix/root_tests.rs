use crate::guest_init::components::nix::root::{
    planned_operations, planned_profile_labels, NixOperation,
};

#[test]
fn nix_bootstrap_operation_order_keeps_nix_blocking() {
    let ops = planned_operations();
    let pos = |op| ops.iter().position(|candidate| candidate == &op).unwrap();
    assert!(pos(NixOperation::BindLower) < pos(NixOperation::MountOverlay));
    assert!(pos(NixOperation::ResizeDisk) < pos(NixOperation::PreseedUpper));
    assert!(pos(NixOperation::PreseedUpper) < pos(NixOperation::MountOverlay));
    assert!(pos(NixOperation::StartDaemon) < pos(NixOperation::WaitSocket));
}

#[test]
fn nix_bootstrap_profile_labels_track_blocking_substeps() {
    let labels = planned_profile_labels();
    let pos = |label| {
        labels
            .iter()
            .position(|candidate| candidate == &label)
            .unwrap()
    };

    assert_eq!(labels.first(), Some(&"bootstrap-nix:require-tools"));
    assert!(pos("bootstrap-nix:bind-lower") < pos("bootstrap-nix:remount-lower-readonly"));
    assert!(pos("bootstrap-nix:resize-disk") < pos("bootstrap-nix:preseed-upper"));
    assert!(pos("bootstrap-nix:preseed-upper") < pos("bootstrap-nix:mount-overlay"));
    assert!(pos("bootstrap-nix:start-daemon") < pos("bootstrap-nix:wait-socket"));
    assert!(labels.contains(&"bootstrap-nix:wait-socket"));
}
