use anyhow::{anyhow, Result};
use clap::Parser;
use std::env;
use std::path::PathBuf;

use crate::podman::image::{podman_image_exists, pull_image};
use crate::runtime::parse_mem_gib_arg;
use crate::{DEFAULT_FALLBACK_IMAGE, DEFAULT_IMAGE};

#[derive(Debug, Parser)]
#[command(
    name = "agentbox",
    version,
    about = "Launch a Podman shell with the current directory mounted at /workspace",
    after_help = "Examples:
  agentbox
  agentbox --pull-latest
  agentbox --native
  agentbox --sidecar-only
  agentbox --sidecar-only --debug
  agentbox --libkrun
  agentbox --mem 8
  agentbox --libkrun-debug-entrypoint ./debug-entrypoint.sh
  agentbox --libkrun-debug-guest-init ./agentbox-guest-init
  agentbox --image ghcr.io/example/agentbox:dev
  AGENTBOX_IMAGE=ghcr.io/example/agentbox:dev agentbox"
)]
pub struct Cli {
    #[arg(
        long,
        env = "AGENTBOX_IMAGE",
        help = "Container image to run",
        long_help = "Container image to run. If omitted, agentbox prefers localhost/agentbox:latest and falls back to ghcr.io/zeroqn/agentbox:latest. Can also be set with AGENTBOX_IMAGE."
    )]
    pub image: Option<String>,

    #[arg(
        long,
        help = "Pull and use ghcr.io/zeroqn/agentbox:latest for this run",
        long_help = "Pull and use ghcr.io/zeroqn/agentbox:latest for this run when --image is not set."
    )]
    pub pull_latest: bool,

    #[arg(
        long,
        help = "Disable sidecar mode (unsupported; seeded fallback has been removed)",
        long_help = "Disable rootless container sidecar mode for this run. This is currently unsupported because the seeded nix fallback has been removed; container mode requires the sidecar. Libkrun mode is the default and does not use this sidecar."
    )]
    pub disable_nix_sidecar: bool,

    #[arg(
        long,
        help = "Start or reuse only the container nix-daemon sidecar stack, then exit",
        long_help = "Start or reuse only the container-mode nix-daemon sidecar stack, skip the nix-daemon socket health probe, print inspection details, and exit without launching the interactive task container. This implicitly selects container mode and leaves the sidecar running for debugging."
    )]
    pub sidecar_only: bool,

    #[arg(
        long,
        help = "Enable Podman debug logging for agentbox-managed Podman commands",
        long_help = "Enable Podman debug logging by passing --log-level=debug to agentbox-managed Podman commands. This is intended for troubleshooting task, sidecar, image, mount, health, and cleanup operations."
    )]
    pub debug: bool,

    #[arg(
        long,
        help = "Use native Podman container mode instead of the default libkrun mode",
        long_help = "Use native Podman container mode with the host-side nix-daemon sidecar instead of the default libkrun mode. This cannot be combined with --libkrun."
    )]
    pub native: bool,

    #[arg(
        long,
        help = "Use default libkrun mode with persistent raw-image /nix overlay",
        long_help = "Use Podman/libkrun mode explicitly. This is the default runtime mode; it creates or reuses a sparse btrfs raw image under agentbox state, attaches it through crun krun.disk annotations, and uses it for a persistent /nix overlay inside the guest."
    )]
    pub libkrun: bool,

    #[arg(
        long,
        help = "Use libkrun TSI/proxy networking instead of default passt",
        long_help = "Libkrun-only networking option. By default libkrun mode enables passt with krun.use_passt=1; --tsi switches to the TSI/proxy environment path. This flag is valid in default libkrun mode and rejected with container mode selectors such as --native or --sidecar-only."
    )]
    pub tsi: bool,

    #[arg(
        long = "mem",
        value_name = "GiB",
        value_parser = parse_mem_gib_arg,
        help = "Set libkrun VM memory in GiB",
        long_help = "Libkrun-only memory option in integer GiB, emitted as a krun.ram_mib annotation. If omitted, agentbox derives a default from host memory. This flag is valid in default libkrun mode and rejected with container mode selectors such as --native or --sidecar-only."
    )]
    pub mem_gib: Option<u32>,

    #[arg(
        long = "libkrun-debug-entrypoint",
        value_name = "PATH",
        help = "Override the image entrypoint in libkrun mode for guest debugging",
        long_help = "Libkrun-only debug option. Bind-mount the host script read-only and use it as the container entrypoint, bypassing the image entrypoint so guest state such as /sys/class/block can be inspected before /nix disk bootstrap."
    )]
    pub libkrun_debug_entrypoint: Option<PathBuf>,

    #[arg(
        long = "libkrun-debug-guest-init",
        value_name = "PATH",
        help = "Override agentbox-guest-init in libkrun mode for guest debugging",
        long_help = "Libkrun-only debug option. Bind-mount the host agentbox-guest-init binary read-only over the in-image guest-init path while preserving the normal image entrypoint and arguments. This lets guest-init fixes be tested without rebuilding the container image."
    )]
    pub libkrun_debug_guest_init: Option<PathBuf>,
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

pub fn resolve_nix_sidecar_enabled(cli: &Cli, env_sidecar_enabled: bool) -> bool {
    if cli.disable_nix_sidecar {
        return false;
    }
    env_sidecar_enabled
}

pub fn env_flag_enabled(name: &str, default: bool) -> Result<bool> {
    match env::var(name) {
        Ok(value) => parse_env_flag_value(name, &value),
        Err(env::VarError::NotPresent) => Ok(default),
        Err(env::VarError::NotUnicode(_)) => Err(anyhow!(
            "environment variable '{}' contains non-UTF-8 data",
            name
        )),
    }
}

fn parse_env_flag_value(name: &str, value: &str) -> Result<bool> {
    let normalized = value.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return Ok(true);
    }

    match normalized.as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        _ => Err(anyhow!(
            "environment variable '{}' must be one of: 1,0,true,false,yes,no,on,off",
            name
        )),
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
