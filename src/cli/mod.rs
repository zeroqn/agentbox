use anyhow::{anyhow, Result};
use clap::Parser;
use std::env;

use crate::podman::image::{podman_image_exists, pull_image};
use crate::{DEFAULT_FALLBACK_IMAGE, DEFAULT_IMAGE};

#[derive(Debug, Parser)]
#[command(
    name = "agentbox",
    version,
    about = "Launch a Podman shell with the current directory mounted at /workspace",
    after_help = "Examples:\n  agentbox\n  agentbox --pull-latest\n  agentbox --disable-nix-sidecar\n  agentbox --image ghcr.io/example/agentbox:dev\n  AGENTBOX_IMAGE=ghcr.io/example/agentbox:dev agentbox"
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
        help = "Disable sidecar mode and run with seeded external nix-state mounts",
        long_help = "Disable rootless sidecar mode for this run and use seeded bind mounts from the resolved external agentbox state root instead."
    )]
    pub disable_nix_sidecar: bool,
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
