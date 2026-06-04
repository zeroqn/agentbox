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

const GUEST_DEBUG_ENV: &str = "LOFTD_GUEST_DEBUG";

pub(crate) fn entrypoint() -> Result<()> {
    debug_breadcrumb("argument parse starting");
    run(GuestInitCli::parse())
}

fn run(cli: GuestInitCli) -> Result<()> {
    debug_breadcrumb("runtime dispatch starting");
    runtime::run(cli.command)
}

fn debug_breadcrumb(message: &str) {
    if guest_debug_enabled_value(std::env::var(GUEST_DEBUG_ENV).ok().as_deref()) {
        eprintln!("loftd-guest-init: debug: {message}");
    }
}

fn guest_debug_enabled_value(value: Option<&str>) -> bool {
    value == Some("1")
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
