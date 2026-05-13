use anyhow::{bail, Result};
use std::path::{Path, PathBuf};

use crate::guest_init::cli::{LibkrunCommand, LibkrunSubcommand, PodmanSubcommand};
use crate::guest_init::components;
use crate::guest_init::components::env::{LibkrunEnv, DEFAULT_SHELL};
use crate::guest_init::components::home::identity::{validate_host_identity, DevIdentity};
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
    BootstrapNix,
    StartPodmanPrep,
    CheckNixSocket,
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
        LibkrunEnterOperation::BootstrapNix,
        LibkrunEnterOperation::StartPodmanPrep,
        LibkrunEnterOperation::CheckNixSocket,
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
        LibkrunSubcommand::Podman(podman) => match podman.command {
            PodmanSubcommand::Prep => components::podman::root::run_prep_to_status(),
            PodmanSubcommand::Wait => components::podman::user::wait_for_prep(),
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
    profiler.measure_result("bootstrap-nix", || {
        crate::guest_init::components::nix::root::bootstrap(&env_contract)
    })?;
    profiler.measure_result("start-podman-prep", || {
        crate::guest_init::components::podman::root::start_background_prep(&identity, &env_contract)
    })?;

    profiler.measure_result("check-nix-socket", || {
        if env_contract.nix_overlay {
            ensure_nix_socket_visible()?;
        }
        Ok(())
    })?;

    profile::clear_guest_profile_env();
    profiler.report_before_exec()?;
    if process::is_root() {
        process::drop_to_identity_and_exec(&identity, &command)
    } else {
        process::exec_command(&command)
    }
}

fn resolve_shell(command: &[String]) -> PathBuf {
    let shell = command.first().map(String::as_str).unwrap_or(DEFAULT_SHELL);
    if shell.contains('/') {
        PathBuf::from(shell)
    } else {
        command::find_on_path(shell).unwrap_or_else(|| PathBuf::from(shell))
    }
}

fn ensure_nix_socket_visible() -> Result<()> {
    let nix_remote = std::env::var("NIX_REMOTE").unwrap_or_default();
    let Some(socket_path) = nix_remote.strip_prefix("unix://") else {
        bail!("libkrun in-guest nix-daemon socket is not configured in NIX_REMOTE");
    };
    if !Path::new(socket_path).exists() {
        bail!("libkrun in-guest nix-daemon socket is not accessible before dropping privileges: {socket_path}");
    }
    Ok(())
}

#[cfg(test)]
#[path = "libkrun_tests.rs"]
mod tests;
