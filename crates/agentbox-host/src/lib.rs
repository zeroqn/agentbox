use anyhow::Result;
use clap::Parser;
use std::fs::File;
use std::io::{self, Read};
use std::path::Path;
use std::process::{self, ExitCode};
use std::time::{SystemTime, UNIX_EPOCH};

mod cli;
mod config;
mod podman;
mod runtime;
mod state;

use cli::Cli;
use podman::process::set_podman_debug;

const DEFAULT_IMAGE: &str = "localhost/agentbox:latest";
const DEFAULT_FALLBACK_IMAGE: &str = "ghcr.io/zeroqn/agentbox:latest";
const CONTAINER_WORKDIR: &str = "/workspace";
const HOST_NIX_UPPER_DIR: &str = "nix-upper";
const HOST_NIX_WORK_DIR: &str = "nix-work";
const HOST_NIX_MERGED_DIR: &str = "nix-merged";
const HOST_NIX_SIDECAR_STATE_FILE: &str = "nix-sidecar.state";
const CONTAINER_CODEX_DIR: &str = "/home/dev/.codex";
const CONTAINER_CARGO_DIR: &str = "/home/dev/.cargo";
const CONTAINER_SCCACHE_DIR: &str = "/home/dev/.cache/sccache";
const CONTAINER_NIX_DIR: &str = "/nix";
const CONTAINER_TMP_TMPFS: &str = "/tmp:rw,exec,mode=1777";
const NIX_STORE_DIR: &str = "store";
const INTERACTIVE_SHELL: &str = "fish";
const NIX_REMOTE_SOCKET: &str = "unix:///nix/var/nix/daemon-socket/socket";
const SIDECAR_NAME_PREFIX: &str = "agentbox-nix-sidecar";
const SIDECAR_NAME_SLUG_FALLBACK: &str = "workspace";
const SIDECAR_NAME_SLUG_MAX_LEN: usize = 32;
const TASK_HOSTNAME_SUFFIX: &str = "agentbox";
const SIDECAR_SOCKET_PATH: &str = "/nix/var/nix/daemon-socket/socket";
const SIDECAR_HEALTH_ATTEMPTS: u32 = 30;
const SIDECAR_HEALTH_DELAY_MS: u64 = 200;
const SIDECAR_LOG_TAIL_LINES: u32 = 120;
const TASK_CONTAINER_ROLE_LABEL: &str = "io.agentbox.role";
const TASK_CONTAINER_ROLE_VALUE: &str = "task";
const TASK_CONTAINER_SIDECAR_LABEL: &str = "io.agentbox.sidecar";

pub fn entrypoint() -> ExitCode {
    let cli = Cli::parse();

    match run(cli) {
        Ok(code) => code,
        Err(err) => {
            eprintln!("agentbox: {err:#}");
            ExitCode::from(1)
        }
    }
}

fn run(cli: Cli) -> Result<ExitCode> {
    set_podman_debug(cli.debug());
    runtime::run(cli)
}

fn derive_task_hostname(cwd: &Path) -> String {
    format!("{}-{TASK_HOSTNAME_SUFFIX}", derive_workspace_slug(cwd))
}

fn derive_task_container_name(cwd: &Path) -> String {
    derive_task_container_name_with_suffix(cwd, &derive_task_container_name_suffix())
}

fn derive_task_container_name_with_suffix(cwd: &Path, suffix: &str) -> String {
    format!("{}-{suffix}", derive_workspace_slug(cwd))
}

fn derive_task_container_name_suffix() -> String {
    if let Ok(suffix) = random_task_container_name_suffix() {
        return suffix;
    }

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();

    format!("{:x}-{timestamp:x}", process::id())
}

fn random_task_container_name_suffix() -> io::Result<String> {
    let mut bytes = [0; 8];
    File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    Ok(format!("{:016x}", u64::from_ne_bytes(bytes)))
}

fn derive_workspace_slug(cwd: &Path) -> String {
    let workspace_name = cwd
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or(SIDECAR_NAME_SLUG_FALLBACK);

    let mut slug = String::new();
    let mut last_was_separator = false;

    for ch in workspace_name.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            last_was_separator = false;
        } else if !slug.is_empty() && !last_was_separator {
            slug.push('-');
            last_was_separator = true;
        }
    }

    let truncated = slug
        .trim_matches('-')
        .chars()
        .take(SIDECAR_NAME_SLUG_MAX_LEN)
        .collect::<String>();
    let trimmed = truncated.trim_matches('-');

    if trimmed.is_empty() {
        SIDECAR_NAME_SLUG_FALLBACK.to_owned()
    } else {
        trimmed.to_owned()
    }
}

#[cfg(test)]
mod tests;
