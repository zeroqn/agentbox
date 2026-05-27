mod container;
mod libkrun;
mod microvm;

use anyhow::Result;
use clap::{Parser, Subcommand};

pub use container::{ContainerMode, ContainerOptions};
pub use libkrun::{
    LibkrunCommand, LibkrunOptions, LibkrunResetNixOptions, LibkrunResizeOptions,
    LibkrunResizeTarget, LibkrunSubcommand,
};
pub use microvm::{MicrovmOptions, MicrovmStoragePolicy};

use crate::podman::image::{podman_image_exists, pull_image};
use crate::{DEFAULT_FALLBACK_IMAGE, DEFAULT_IMAGE};

#[derive(Debug, Parser)]
#[command(
    name = "agentbox",
    version,
    about = "Launch a Podman shell with the current directory mounted at /workspace",
    after_help = "Examples:
  agentbox
  agentbox libkrun
  agentbox libkrun --mem 8
  agentbox libkrun resize --target nix --size 128G
  agentbox libkrun reset-nix --force
  agentbox libkrun --guest-init ./agentbox-guest-init
  agentbox container
  agentbox container sidecar
  agentbox microvm --help
  agentbox microvm --storage auto
  agentbox --debug container sidecar
  agentbox container --debug sidecar
  agentbox --profile --debug
  agentbox --root
  agentbox --root container
  agentbox --image ghcr.io/example/agentbox:dev container
  AGENTBOX_IMAGE=ghcr.io/example/agentbox:dev agentbox"
)]
pub struct Cli {
    #[arg(
        long,
        env = "AGENTBOX_IMAGE",
        global = true,
        help = "Container image to run",
        long_help = "Container image to run. If omitted, agentbox prefers localhost/agentbox:latest and falls back to ghcr.io/zeroqn/agentbox:latest. Can also be set with AGENTBOX_IMAGE."
    )]
    image: Option<String>,

    #[arg(
        long,
        global = true,
        help = "Pull and use ghcr.io/zeroqn/agentbox:latest for this run",
        long_help = "Pull and use ghcr.io/zeroqn/agentbox:latest for this run when --image is not set."
    )]
    pull_latest: bool,

    #[arg(
        long,
        global = true,
        help = "Enable Podman debug logging for agentbox-managed Podman commands",
        long_help = "Enable Podman debug logging by passing --log-level=debug to agentbox-managed Podman commands. This is intended for troubleshooting task, sidecar, image, mount, health, and cleanup operations."
    )]
    debug: bool,

    #[arg(
        long,
        global = true,
        help = "Enable guest-init component timing collection",
        long_help = "Enable agentbox-guest-init component timing collection for the task container. Timing is reported to stderr only when --debug is also set, so normal command stdout remains reserved for command output."
    )]
    profile: bool,

    #[arg(
        long,
        global = true,
        help = "Enter the task shell as root",
        long_help = "Enter the task shell as root instead of dropping to the host/dev identity. By default, agentbox drops privileges for the interactive shell."
    )]
    root: bool,

    #[command(subcommand)]
    command: Option<CliCommand>,
}

impl Cli {
    pub fn debug(&self) -> bool {
        self.debug
    }

    #[cfg(test)]
    pub fn common_options(&self) -> CommonOptions {
        CommonOptions {
            image: self.image.clone(),
            pull_latest: self.pull_latest,
            debug: self.debug,
            profile: self.profile,
            root: self.root,
        }
    }

    #[cfg(test)]
    pub fn runtime_command_or_default(&self) -> RuntimeCommand {
        self.command
            .clone()
            .map(RuntimeCommand::from)
            .unwrap_or_else(|| RuntimeCommand::Libkrun(LibkrunCommand::default()))
    }

    pub fn into_runtime_parts(self) -> (CommonOptions, RuntimeCommand) {
        let common = CommonOptions {
            image: self.image,
            pull_latest: self.pull_latest,
            debug: self.debug,
            profile: self.profile,
            root: self.root,
        };
        let command = self
            .command
            .map(RuntimeCommand::from)
            .unwrap_or_else(|| RuntimeCommand::Libkrun(LibkrunCommand::default()));

        (common, command)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommonOptions {
    pub image: Option<String>,
    pub pull_latest: bool,
    pub debug: bool,
    pub profile: bool,
    pub root: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeCommand {
    Libkrun(LibkrunCommand),
    Container(ContainerOptions),
    Microvm(MicrovmOptions),
}

#[derive(Debug, Clone, PartialEq, Eq, Subcommand)]
enum CliCommand {
    #[command(about = "Run the default Podman/libkrun VM-backed container shell")]
    Libkrun(LibkrunCommand),
    #[command(about = "Run native Podman container mode with the managed nix-daemon sidecar")]
    Container(ContainerOptions),
    #[command(about = "Run experimental direct-libkrun microvm mode")]
    Microvm(MicrovmOptions),
}

impl From<CliCommand> for RuntimeCommand {
    fn from(command: CliCommand) -> Self {
        match command {
            CliCommand::Libkrun(options) => RuntimeCommand::Libkrun(options),
            CliCommand::Container(options) => RuntimeCommand::Container(options),
            CliCommand::Microvm(options) => RuntimeCommand::Microvm(options),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ImageResolutionStrategy {
    Explicit(String),
    PullLatestGhcr,
    PreferLocalhostFallback,
}

pub fn resolve_image(cli_image: Option<&str>, pull_latest: bool) -> Result<String> {
    match resolve_image_strategy(cli_image, pull_latest) {
        ImageResolutionStrategy::Explicit(image) => Ok(image),
        ImageResolutionStrategy::PullLatestGhcr => {
            pull_image(DEFAULT_FALLBACK_IMAGE)?;
            Ok(DEFAULT_FALLBACK_IMAGE.to_owned())
        }
        ImageResolutionStrategy::PreferLocalhostFallback => {
            let localhost_available = podman_image_exists(DEFAULT_IMAGE)?;
            Ok(select_default_image(localhost_available).to_owned())
        }
    }
}

fn resolve_image_strategy(cli_image: Option<&str>, pull_latest: bool) -> ImageResolutionStrategy {
    if let Some(image) = cli_image {
        return ImageResolutionStrategy::Explicit(image.to_owned());
    }

    if pull_latest {
        return ImageResolutionStrategy::PullLatestGhcr;
    }

    ImageResolutionStrategy::PreferLocalhostFallback
}

fn select_default_image(localhost_available: bool) -> &'static str {
    if localhost_available {
        DEFAULT_IMAGE
    } else {
        DEFAULT_FALLBACK_IMAGE
    }
}

#[cfg(test)]
mod tests;
