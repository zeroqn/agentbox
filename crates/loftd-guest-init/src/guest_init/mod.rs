use anyhow::bail;
use clap::Parser;

mod cli;

use cli::{GuestInitCli, GuestInitCommand};

pub(crate) fn entrypoint() -> anyhow::Result<()> {
    match GuestInitCli::parse().command {
        GuestInitCommand::Enter(command) => {
            let resolved = command.resolved_command();
            bail!(
                "loftd-guest-init guest bootstrap is not implemented yet (requested command: {})",
                resolved.join(" ")
            )
        }
    }
}
