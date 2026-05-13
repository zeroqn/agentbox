use crate::guest_init::runtime::libkrun::{planned_enter_operations, LibkrunEnterOperation};

#[test]
fn libkrun_enter_operation_order_keeps_components_before_exec() {
    let ops = planned_enter_operations();
    let pos = |op| ops.iter().position(|candidate| candidate == &op).unwrap();
    assert!(
        pos(LibkrunEnterOperation::DeriveShellEnvironment)
            < pos(LibkrunEnterOperation::StartPodmanPrep)
    );
    assert!(
        pos(LibkrunEnterOperation::ExportShellEnvironment)
            < pos(LibkrunEnterOperation::MaterializeHome)
    );
    assert!(pos(LibkrunEnterOperation::BootstrapNix) < pos(LibkrunEnterOperation::DropAndExec));
    assert!(pos(LibkrunEnterOperation::StartPodmanPrep) < pos(LibkrunEnterOperation::DropAndExec));
}
