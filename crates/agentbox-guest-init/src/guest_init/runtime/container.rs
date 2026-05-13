use anyhow::{bail, Result};

use crate::guest_init::cli::{ContainerCommand, ContainerSubcommand, EnterCommand};

pub(in crate::guest_init) fn run(command: ContainerCommand) -> Result<()> {
    match command.command {
        ContainerSubcommand::Enter(enter_command) => enter(enter_command),
    }
}

fn enter(_command: EnterCommand) -> Result<()> {
    bail!("agentbox-guest-init container enter is parser-compatible but not wired in this image")
}
