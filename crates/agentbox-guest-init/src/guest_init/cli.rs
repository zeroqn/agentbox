use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser, PartialEq, Eq)]
#[command(name = "agentbox-guest-init")]
pub(in crate::guest_init) struct GuestInitCli {
    #[command(subcommand)]
    pub(in crate::guest_init) runtime: RuntimeCommand,
}

#[derive(Debug, Subcommand, PartialEq, Eq)]
pub(in crate::guest_init) enum RuntimeCommand {
    Default(DefaultCommand),
    Libkrun(LibkrunCommand),
    Container(ContainerCommand),
}

#[derive(Debug, Args, PartialEq, Eq)]
pub(in crate::guest_init) struct DefaultCommand {
    #[command(subcommand)]
    pub(in crate::guest_init) command: DefaultSubcommand,
}

#[derive(Debug, Subcommand, PartialEq, Eq)]
pub(in crate::guest_init) enum DefaultSubcommand {
    Enter(EnterCommand),
}

#[derive(Debug, Args, PartialEq, Eq)]
pub(in crate::guest_init) struct LibkrunCommand {
    #[command(subcommand)]
    pub(in crate::guest_init) command: LibkrunSubcommand,
}

#[derive(Debug, Subcommand, PartialEq, Eq)]
pub(in crate::guest_init) enum LibkrunSubcommand {
    Enter(EnterCommand),
    Nix(NixCommand),
    Podman(PodmanCommand),
}

#[derive(Debug, Args, PartialEq, Eq)]
pub(in crate::guest_init) struct ContainerCommand {
    #[command(subcommand)]
    pub(in crate::guest_init) command: ContainerSubcommand,
}

#[derive(Debug, Subcommand, PartialEq, Eq)]
pub(in crate::guest_init) enum ContainerSubcommand {
    Enter(EnterCommand),
}

#[derive(Debug, Args, Clone, PartialEq, Eq)]
pub(in crate::guest_init) struct EnterCommand {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub(in crate::guest_init) command: Vec<String>,
}

#[derive(Debug, Args, PartialEq, Eq)]
pub(in crate::guest_init) struct PodmanCommand {
    #[command(subcommand)]
    pub(in crate::guest_init) command: PodmanSubcommand,
}

#[derive(Debug, Subcommand, PartialEq, Eq)]
pub(in crate::guest_init) enum PodmanSubcommand {
    Prep,
    Wait,
}

#[derive(Debug, Args, PartialEq, Eq)]
pub(in crate::guest_init) struct NixCommand {
    #[command(subcommand)]
    pub(in crate::guest_init) command: NixSubcommand,
}

#[derive(Debug, Subcommand, PartialEq, Eq)]
pub(in crate::guest_init) enum NixSubcommand {
    Prep,
    Wait,
}

impl EnterCommand {
    pub(in crate::guest_init) fn resolved_command(&self) -> Vec<String> {
        if self.command.is_empty() {
            vec!["fish".to_owned(), "-l".to_owned()]
        } else {
            self.command.clone()
        }
    }
}

#[cfg(test)]
#[path = "cli_tests.rs"]
mod tests;
