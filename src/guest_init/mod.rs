use anyhow::{bail, Result};
use clap::Parser;
use std::path::PathBuf;

mod cli;
mod command;
mod fs;
mod process;
mod root;
mod runtime;
mod status;
mod user;

use cli::{ContainerSubcommand, GuestInitCli, LibkrunSubcommand, PodmanSubcommand, RuntimeCommand};

pub fn entrypoint() -> Result<()> {
    run(GuestInitCli::parse())
}

fn run(cli: GuestInitCli) -> Result<()> {
    match cli.runtime {
        RuntimeCommand::Libkrun(libkrun) => match libkrun.command {
            LibkrunSubcommand::Enter(enter) => enter_libkrun(enter.resolved_command()),
            LibkrunSubcommand::Podman(podman) => match podman.command {
                PodmanSubcommand::Prep => root::podman::run_prep_to_status(),
                PodmanSubcommand::Wait => user::podman::wait_for_prep(),
            },
        },
        RuntimeCommand::Container(container) => match container.command {
            ContainerSubcommand::Enter(enter) => runtime::container::enter(enter),
        },
    }
}

fn enter_libkrun(command: Vec<String>) -> Result<()> {
    let env_contract = runtime::libkrun::LibkrunEnv::from_process_env()?;
    let (uid, gid) = if process::is_root() {
        let (uid, gid) = env_contract.require_host_identity()?;
        root::home::validate_host_identity(uid, gid)?;
        (uid, gid)
    } else {
        (process::uid(), process::gid())
    };
    let shell = resolve_shell(&command);
    let identity = root::home::DevIdentity::new(uid, gid, shell);
    let shell_env =
        runtime::libkrun::derive_shell_environment(&identity, env_contract.containers_storage);

    runtime::libkrun::export_shell_environment(&shell_env);
    root::home::materialize(&identity)?;
    if env_contract.use_passt {
        runtime::libkrun::ensure_passt_resolv_conf(std::path::Path::new("/etc/resolv.conf"))?;
    }
    root::nix::bootstrap(&env_contract)?;
    root::podman::start_background_prep(&identity, &env_contract)?;

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
    let shell = command
        .first()
        .map(String::as_str)
        .unwrap_or(runtime::libkrun::DEFAULT_SHELL);
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
    if !std::path::Path::new(socket_path).exists() {
        bail!("libkrun in-guest nix-daemon socket is not accessible before dropping privileges: {socket_path}");
    }
    Ok(())
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
