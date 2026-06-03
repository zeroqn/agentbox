use anyhow::Result;
use clap::Parser;

mod cli;
mod command;
mod components;
mod fs;
mod process;
mod profile;
mod runtime;

use cli::GuestInitCli;

pub(crate) fn entrypoint() -> Result<()> {
    run(GuestInitCli::parse())
}

fn run(cli: GuestInitCli) -> Result<()> {
    runtime::run(cli.command)
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
