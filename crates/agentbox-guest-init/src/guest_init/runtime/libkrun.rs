use anyhow::{bail, Result};
use std::path::{Path, PathBuf};

use crate::guest_init::components::env::{LibkrunEnv, DEFAULT_SHELL};
use crate::guest_init::components::home::identity::{validate_host_identity, DevIdentity};
use crate::guest_init::{command, process};

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
        LibkrunEnterOperation::DropAndExec,
    ]
}

pub(in crate::guest_init) fn enter(command: Vec<String>) -> Result<()> {
    let env_contract = LibkrunEnv::from_process_env()?;
    let (uid, gid) = if process::is_root() {
        let (uid, gid) = env_contract.require_host_identity()?;
        validate_host_identity(uid, gid)?;
        (uid, gid)
    } else {
        (process::uid(), process::gid())
    };
    let shell = resolve_shell(&command);
    let identity = DevIdentity::new(uid, gid, shell);
    let shell_env = crate::guest_init::components::shell::env::derive(
        &identity,
        env_contract.containers_storage,
    );

    crate::guest_init::components::shell::env::export(&shell_env);
    crate::guest_init::components::home::root::materialize(&identity)?;
    if env_contract.use_passt {
        crate::guest_init::components::net::dns::ensure_passt_resolv_conf(Path::new(
            "/etc/resolv.conf",
        ))?;
    }
    crate::guest_init::components::nix::root::bootstrap(&env_contract)?;
    crate::guest_init::components::podman::root::start_background_prep(&identity, &env_contract)?;

    if env_contract.nix_overlay {
        ensure_nix_socket_visible()?;
    }

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
