use clap::{Args, Subcommand};

#[derive(Debug, Clone, Default, PartialEq, Eq, Args)]
pub struct ContainerOptions {
    #[command(subcommand)]
    command: Option<ContainerCommand>,
}

impl ContainerOptions {
    pub fn mode(&self) -> ContainerMode {
        match self.command.as_ref() {
            Some(ContainerCommand::Sidecar(_)) => ContainerMode::Sidecar,
            None => ContainerMode::Task,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Subcommand)]
enum ContainerCommand {
    #[command(about = "Start or reuse only the container nix-daemon sidecar stack, then exit")]
    Sidecar(SidecarOptions),
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Args)]
struct SidecarOptions {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainerMode {
    Task,
    Sidecar,
}
