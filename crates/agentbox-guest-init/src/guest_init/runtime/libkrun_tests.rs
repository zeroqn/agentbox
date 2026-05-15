use crate::guest_init::cli::{
    DockerCommand, DockerSubcommand, EnterCommand, LibkrunSubcommand, NixCommand, NixSubcommand,
    PodmanCommand, PodmanSubcommand,
};
use crate::guest_init::runtime::libkrun::{
    LibkrunEnterOperation, planned_enter_operations, should_drop_to_identity,
    subcommand_starts_profiler,
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
    assert!(
        pos(LibkrunEnterOperation::FixPasstDns)
            < pos(LibkrunEnterOperation::MaterializeAllocatorPreload)
    );
    assert!(
        pos(LibkrunEnterOperation::MaterializeAllocatorPreload)
            < pos(LibkrunEnterOperation::RestrictDmesg)
    );
    assert!(pos(LibkrunEnterOperation::RestrictDmesg) < pos(LibkrunEnterOperation::StartNixPrep));
    assert!(
        pos(LibkrunEnterOperation::RestrictDmesg) < pos(LibkrunEnterOperation::StartPodmanPrep)
    );
    assert!(
        pos(LibkrunEnterOperation::RestrictDmesg) < pos(LibkrunEnterOperation::StartDockerPrep)
    );
    assert!(pos(LibkrunEnterOperation::RestrictDmesg) < pos(LibkrunEnterOperation::DropAndExec));
    assert!(pos(LibkrunEnterOperation::StartNixPrep) < pos(LibkrunEnterOperation::DropAndExec));
    assert!(pos(LibkrunEnterOperation::StartPodmanPrep) < pos(LibkrunEnterOperation::DropAndExec));
    assert!(pos(LibkrunEnterOperation::StartDockerPrep) < pos(LibkrunEnterOperation::DropAndExec));
    assert!(
        pos(LibkrunEnterOperation::StartPodmanPrep) < pos(LibkrunEnterOperation::StartDockerPrep)
    );
    assert!(pos(LibkrunEnterOperation::StartNixPrep) < pos(LibkrunEnterOperation::StartPodmanPrep));
    assert!(pos(LibkrunEnterOperation::ExportNixRemote) < pos(LibkrunEnterOperation::DropAndExec));
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

#[test]
fn libkrun_root_mode_skips_only_final_identity_drop() {
    assert!(should_drop_to_identity(true, false));
    assert!(!should_drop_to_identity(true, true));
    assert!(!should_drop_to_identity(false, false));
    assert!(!should_drop_to_identity(false, true));
}

#[test]
fn libkrun_docker_subcommands_are_not_profiled_entrypoints() {
    let prep = LibkrunSubcommand::Docker(DockerCommand {
        command: DockerSubcommand::Prep,
    });
    let wait = LibkrunSubcommand::Docker(DockerCommand {
        command: DockerSubcommand::Wait,
    });
    let daemon = LibkrunSubcommand::Docker(DockerCommand {
        command: DockerSubcommand::Daemon,
    });

    assert!(!subcommand_starts_profiler(&prep));
    assert!(!subcommand_starts_profiler(&wait));
    assert!(!subcommand_starts_profiler(&daemon));
}

#[test]
fn libkrun_nix_subcommands_are_not_profiled_entrypoints() {
    let prep = LibkrunSubcommand::Nix(NixCommand {
        command: NixSubcommand::Prep,
    });
    let wait = LibkrunSubcommand::Nix(NixCommand {
        command: NixSubcommand::Wait,
    });

    assert!(!subcommand_starts_profiler(&prep));
    assert!(!subcommand_starts_profiler(&wait));
}
