use anyhow::Result;
use clap::Parser;

mod cli;
mod command;
mod components;
mod fs;
mod process;
mod runtime;

use cli::{ContainerSubcommand, GuestInitCli, LibkrunSubcommand, PodmanSubcommand, RuntimeCommand};

pub fn entrypoint() -> Result<()> {
    run(GuestInitCli::parse())
}

fn run(cli: GuestInitCli) -> Result<()> {
    match cli.runtime {
        RuntimeCommand::Libkrun(libkrun) => match libkrun.command {
            LibkrunSubcommand::Enter(enter) => runtime::libkrun::enter(enter.resolved_command()),
            LibkrunSubcommand::Podman(podman) => match podman.command {
                PodmanSubcommand::Prep => components::podman::root::run_prep_to_status(),
                PodmanSubcommand::Wait => components::podman::user::wait_for_prep(),
            },
        },
        RuntimeCommand::Container(container) => match container.command {
            ContainerSubcommand::Enter(enter) => runtime::container::enter(enter),
        },
    }
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
