use anyhow::Result;
use clap::Parser;
use std::ffi::OsString;
use std::process::ExitCode;
use std::time::Instant;

mod cli;
mod config;
mod logging;
mod naming;
mod runtime;
mod state;
mod task_rootfs;

use cli::{Cli, CliAction, RuntimeOptions};

const DEFAULT_IMAGE: &str = "localhost/loftd:latest";
const DEFAULT_FALLBACK_IMAGE: &str = "ghcr.io/zeroqn/loftd:latest";

pub fn entrypoint() -> ExitCode {
    let host_session_started_at = Instant::now();
    let args = std::env::args_os().collect::<Vec<_>>();
    if is_internal_invocation(&args) {
        return match run_internal(args.into_iter().skip(2).collect()) {
            Ok(code) => code,
            Err(err) => {
                eprintln!("loftd internal: {err:#}");
                ExitCode::from(1)
            }
        };
    }

    match Cli::parse().into_action() {
        CliAction::Run(options) => run_with_logging(
            options,
            runtime::RuntimeProfileScope::from_started_at(host_session_started_at),
        ),
        CliAction::DecodeLaunchConf { path } => {
            match runtime::launch::config::LaunchConfig::decode_file_for_debug(&path) {
                Ok(decoded) => {
                    print!("{decoded}");
                    ExitCode::SUCCESS
                }
                Err(err) => {
                    eprintln!("loftd: {err:#}");
                    ExitCode::from(1)
                }
            }
        }
    }
}

fn run_with_logging(
    options: RuntimeOptions,
    profile_scope: runtime::RuntimeProfileScope,
) -> ExitCode {
    if let Err(err) = logging::init_tracing(&options.log_settings) {
        eprintln!("loftd: {err:#}");
        return ExitCode::from(1);
    }

    match run(options, profile_scope) {
        Ok(code) => code,
        Err(err) => {
            eprintln!("loftd: {err:#}");
            ExitCode::from(1)
        }
    }
}

fn run(options: RuntimeOptions, profile_scope: runtime::RuntimeProfileScope) -> Result<ExitCode> {
    runtime::run(options, profile_scope)
}

fn is_internal_invocation(args: &[OsString]) -> bool {
    args.get(1).and_then(|arg| arg.to_str()) == Some("internal")
}

fn run_internal(args: Vec<OsString>) -> Result<ExitCode> {
    runtime::run_internal(args)?;
    Ok(ExitCode::SUCCESS)
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
