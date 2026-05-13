use crate::guest_init::cli::{EnterCommand, LibkrunSubcommand, PodmanCommand, PodmanSubcommand};
use crate::guest_init::runtime::libkrun::{
    planned_enter_operations, subcommand_starts_profiler, LibkrunEnterOperation,
};

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
    assert!(
        pos(LibkrunEnterOperation::ClearProfileEnvBeforeExec)
            < pos(LibkrunEnterOperation::DropAndExec)
    );
    assert!(
        pos(LibkrunEnterOperation::ReportProfileBeforeExec)
            < pos(LibkrunEnterOperation::DropAndExec)
    );
}

#[test]
fn libkrun_podman_subcommands_are_not_profiled_entrypoints() {
    let enter = LibkrunSubcommand::Enter(EnterCommand {
        command: vec!["fish".to_owned(), "-l".to_owned()],
    });
    let prep = LibkrunSubcommand::Podman(PodmanCommand {
        command: PodmanSubcommand::Prep,
    });
    let wait = LibkrunSubcommand::Podman(PodmanCommand {
        command: PodmanSubcommand::Wait,
    });

    assert!(subcommand_starts_profiler(&enter));
    assert!(!subcommand_starts_profiler(&prep));
    assert!(!subcommand_starts_profiler(&wait));
}
