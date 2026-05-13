use crate::guest_init::components::nix::root::{planned_operations, NixOperation};

#[test]
fn nix_bootstrap_operation_order_keeps_nix_blocking() {
    let ops = planned_operations();
    let pos = |op| ops.iter().position(|candidate| candidate == &op).unwrap();
    assert!(pos(NixOperation::BindLower) < pos(NixOperation::MountOverlay));
    assert!(pos(NixOperation::ResizeDisk) < pos(NixOperation::PreseedUpper));
    assert!(pos(NixOperation::PreseedUpper) < pos(NixOperation::MountOverlay));
    assert!(pos(NixOperation::StartDaemon) < pos(NixOperation::WaitSocket));
}
