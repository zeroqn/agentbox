use anyhow::Result;
use clap::Parser;
use std::process::ExitCode;

mod cli;
mod config;
mod naming;
mod runtime;
mod state;

use cli::Cli;

const DEFAULT_IMAGE: &str = "localhost/loftd:latest";
const DEFAULT_FALLBACK_IMAGE: &str = "ghcr.io/zeroqn/loftd:latest";

pub fn entrypoint() -> ExitCode {
    let cli = Cli::parse();

    match run(cli) {
        Ok(code) => code,
        Err(err) => {
            eprintln!("loftd: {err:#}");
            ExitCode::from(1)
        }
    }
}

fn run(cli: Cli) -> Result<ExitCode> {
    runtime::run(cli)
}

#[cfg(test)]
mod tests {
    use clap::{CommandFactory, Parser};

    use crate::cli::Cli;

    #[test]
    fn command_help_renders() {
        Cli::command().debug_assert();
    }

    #[test]
    fn top_level_cli_does_not_accept_microvm_subcommand() {
        let err = Cli::try_parse_from(["loftd", "microvm"]).expect_err("microvm is not a command");

        assert_eq!(err.kind(), clap::error::ErrorKind::UnknownArgument);
    }
}
