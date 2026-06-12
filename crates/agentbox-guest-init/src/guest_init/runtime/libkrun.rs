use anyhow::Result;
use std::path::{Path, PathBuf};

use crate::guest_init::cli::{LibkrunCommand, LibkrunSubcommand, NixSubcommand, PodmanSubcommand};
use crate::guest_init::components;
use crate::guest_init::components::env::{DEFAULT_SHELL, LibkrunEnv, NIX_REMOTE_URI};
use crate::guest_init::components::home::identity::{DevIdentity, validate_host_identity};
use crate::guest_init::{command, process, profile};

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::guest_init) enum LibkrunEnterOperation {
    ReadEnv,
    ResolveIdentity,
    DeriveShellEnvironment,
    ExportShellEnvironment,
    MaterializeHome,
    FixPasstDns,
    MaterializeAllocatorPreload,
    RestrictDmesg,
    StartNixPrep,
    StartPodmanPrep,
    ExportNixRemote,
    EnsureNofileFloor,
    ClearProfileEnvBeforeExec,
    ReportProfileBeforeExec,
    DropAndExec,
}

#[cfg(test)]
pub(in crate::guest_init) fn planned_enter_operations() -> Vec<LibkrunEnterOperation> {
    vec![
        LibkrunEnterOperation::ReadEnv,
        LibkrunEnterOperation::ResolveIdentity,
        LibkrunEnterOperation::DeriveShellEnvironment,
        LibkrunEnterOperation::ExportShellEnvironment,
        LibkrunEnterOperation::MaterializeHome,
        LibkrunEnterOperation::FixPasstDns,
        LibkrunEnterOperation::MaterializeAllocatorPreload,
        LibkrunEnterOperation::RestrictDmesg,
        LibkrunEnterOperation::StartNixPrep,
        LibkrunEnterOperation::StartPodmanPrep,
        LibkrunEnterOperation::ExportNixRemote,
        LibkrunEnterOperation::EnsureNofileFloor,
        LibkrunEnterOperation::ClearProfileEnvBeforeExec,
        LibkrunEnterOperation::ReportProfileBeforeExec,
        LibkrunEnterOperation::DropAndExec,
    ]
}

#[cfg(test)]
pub(in crate::guest_init) fn subcommand_starts_profiler(command: &LibkrunSubcommand) -> bool {
    matches!(command, LibkrunSubcommand::Enter(_))
}

pub(in crate::guest_init) fn run(command: LibkrunCommand) -> Result<()> {
    match command.command {
        LibkrunSubcommand::Enter(enter_command) => enter(enter_command.resolved_command()),
        LibkrunSubcommand::Resize(resize) => components::disk::resize::run(resize.target),
        LibkrunSubcommand::Nix(nix) => match nix.command {
            NixSubcommand::Prep => components::nix::root::run_prep_to_status(),
            NixSubcommand::Wait => components::nix::user::wait_for_prep(),
        },
        LibkrunSubcommand::Podman(podman) => match podman.command {
            PodmanSubcommand::Prep => components::podman::root::run_prep_to_status(),
            PodmanSubcommand::Wait => components::podman::user::wait_for_prep(),
            PodmanSubcommand::ServiceWait => components::podman::user::wait_for_service(),
        },
    }
}

fn enter(command: Vec<String>) -> Result<()> {
    let mut profiler = profile::GuestProfiler::from_process_env("libkrun enter");
    let env_contract = profiler.measure_result("read-env", LibkrunEnv::from_process_env)?;
    let (uid, gid) = profiler.measure_result("resolve-identity", || {
        if process::is_root() {
            let (uid, gid) = env_contract.require_host_identity()?;
            validate_host_identity(uid, gid)?;
            Ok((uid, gid))
        } else {
            Ok((process::uid(), process::gid()))
        }
    })?;
    let shell = resolve_shell(&command);
    let identity = DevIdentity::new(uid, gid, shell);
    let shell_env = profiler.measure("derive-shell-env", || {
        crate::guest_init::components::shell::env::derive(
            &identity,
            env_contract.containers_storage,
        )
    });

    profiler.measure("export-shell-env", || {
        crate::guest_init::components::shell::env::export(&shell_env);
    });
    profiler.measure_result("materialize-home", || {
        crate::guest_init::components::home::root::materialize(&identity)
    })?;
    profiler.measure_result("fix-passt-dns", || {
        if env_contract.use_passt {
            crate::guest_init::components::net::dns::ensure_passt_resolv_conf(Path::new(
                "/etc/resolv.conf",
            ))?;
        }
        Ok(())
    })?;
    profiler.measure_result("materialize-allocator-preload", || {
        crate::guest_init::components::hardening::allocator::ensure_from_env_if_root(
            process::is_root(),
        )
    })?;
    profiler.measure_result("restrict-dmesg", || {
        if process::is_root() {
            crate::guest_init::components::hardening::dmesg::restrict()?;
        }
        Ok(())
    })?;
    profiler.measure_result("start-nix-prep", || {
        crate::guest_init::components::nix::root::start_background_prep(&env_contract)
    })?;
    profiler.measure_result("start-podman-prep", || {
        crate::guest_init::components::podman::root::start_background_prep(&identity, &env_contract)
    })?;
    profiler.measure("export-nix-remote", || {
        if env_contract.nix_overlay {
            // SAFETY: libkrun entry mutates the process environment during
            // single-threaded bootstrap before exec so the shell sees NIX_REMOTE.
            unsafe { std::env::set_var("NIX_REMOTE", NIX_REMOTE_URI) };
        }
    });
    profiler.measure_result("ensure-nofile-floor", process::ensure_nofile_floor)?;

    profile::clear_guest_profile_env();
    profiler.report_before_exec()?;
    if should_drop_to_identity(process::is_root(), env_contract.enter_as_root) {
        process::drop_to_identity_and_exec(&identity, &command)
    } else {
        process::exec_command(&command)
    }
}

pub(in crate::guest_init) fn should_drop_to_identity(is_root: bool, enter_as_root: bool) -> bool {
    is_root && !enter_as_root
}

fn resolve_shell(command: &[String]) -> PathBuf {
    let shell = command.first().map(String::as_str).unwrap_or(DEFAULT_SHELL);
    if shell.contains('/') {
        PathBuf::from(shell)
    } else {
        command::find_on_path(shell).unwrap_or_else(|| PathBuf::from(shell))
    }
}

#[cfg(test)]
#[path = "libkrun_tests.rs"]
mod tests;
