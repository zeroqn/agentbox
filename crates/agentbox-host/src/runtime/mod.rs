mod components;
mod container;
mod libkrun;
pub(crate) mod microvm;

use anyhow::Result;
use std::process::ExitCode;

use crate::cli::{Cli, RuntimeCommand};

pub(crate) use libkrun::{parse_mem_gib_arg, parse_raw_image_size_arg};

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RuntimeMode {
    Container,
    Libkrun,
    Microvm,
}

pub(crate) fn run(cli: Cli) -> Result<ExitCode> {
    let (common, command) = cli.into_runtime_parts();
    match command {
        RuntimeCommand::Container(options) => container::run(common, options),
        RuntimeCommand::Libkrun(options) => libkrun::run(common, options),
        RuntimeCommand::Microvm(options) => microvm::run(common, options),
    }
}

#[cfg(test)]
fn resolve_runtime_mode(cli: &Cli) -> RuntimeMode {
    match cli.runtime_command_or_default() {
        RuntimeCommand::Container(_) => RuntimeMode::Container,
        RuntimeCommand::Libkrun(_) => RuntimeMode::Libkrun,
        RuntimeCommand::Microvm(_) => RuntimeMode::Microvm,
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use crate::cli::Cli;
    use crate::runtime::{RuntimeMode, resolve_runtime_mode};

    fn parse(args: &[&str]) -> Cli {
        let mut argv = vec!["agentbox"];
        argv.extend(args.iter().copied());
        Cli::parse_from(argv)
    }

    #[test]
    fn default_mode_is_libkrun() {
        assert_eq!(resolve_runtime_mode(&parse(&[])), RuntimeMode::Libkrun);
    }

    #[test]
    fn libkrun_subcommand_resolves_to_libkrun() {
        assert_eq!(
            resolve_runtime_mode(&parse(&["libkrun"])),
            RuntimeMode::Libkrun
        );
    }

    #[test]
    fn container_subcommand_resolves_to_container() {
        assert_eq!(
            resolve_runtime_mode(&parse(&["container"])),
            RuntimeMode::Container
        );
    }

    #[test]
    fn container_sidecar_subcommand_resolves_to_container() {
        assert_eq!(
            resolve_runtime_mode(&parse(&["container", "sidecar"])),
            RuntimeMode::Container
        );
    }

    #[test]
    fn microvm_subcommand_resolves_to_microvm() {
        assert_eq!(
            resolve_runtime_mode(&parse(&["microvm"])),
            RuntimeMode::Microvm
        );
    }

    #[test]
    fn microvm_boot_pending_error_is_explicit() {
        let message = crate::runtime::microvm::boot_pending_message().to_lowercase();

        assert!(message.contains("microvm"));
        assert!(message.contains("experimental"));
        assert!(message.contains("direct"));
        assert!(message.contains("enabled"));
    }

    #[test]
    fn microvm_pull_latest_error_rejects_podman_backed_semantics() {
        let message = crate::runtime::microvm::pull_latest_not_supported_message().to_lowercase();

        assert!(message.contains("pull-latest"));
        assert!(message.contains("microvm"));
        assert!(message.contains("podman"));
    }
}
