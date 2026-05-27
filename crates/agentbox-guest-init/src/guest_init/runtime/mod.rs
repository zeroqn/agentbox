pub(in crate::guest_init) mod container;
pub(in crate::guest_init) mod default;
pub(in crate::guest_init) mod libkrun;
pub(in crate::guest_init) mod microvm;

use anyhow::Result;

use crate::guest_init::cli::RuntimeCommand;

pub(in crate::guest_init) fn run(command: RuntimeCommand) -> Result<()> {
    match command {
        RuntimeCommand::Default(command) => default::run(command),
        RuntimeCommand::Libkrun(command) => libkrun::run(command),
        RuntimeCommand::Container(command) => container::run(command),
        RuntimeCommand::Microvm(command) => microvm::run(command),
    }
}
