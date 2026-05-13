pub(in crate::guest_init) mod container;
pub(in crate::guest_init) mod libkrun;

use anyhow::Result;

use crate::guest_init::cli::RuntimeCommand;

pub(in crate::guest_init) fn run(command: RuntimeCommand) -> Result<()> {
    match command {
        RuntimeCommand::Libkrun(command) => libkrun::run(command),
        RuntimeCommand::Container(command) => container::run(command),
    }
}
