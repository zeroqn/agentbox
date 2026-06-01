use anyhow::Result;
use clap::Parser;
use std::process::ExitCode;

mod cli;
mod config;
mod naming;
mod podman;
mod runtime;
mod state;

use cli::Cli;
use podman::process::set_podman_debug;
use runtime::microvm::supervisor::MICROVM_HELPER_ARG;

const DEFAULT_IMAGE: &str = "localhost/loftd:latest";
const DEFAULT_FALLBACK_IMAGE: &str = "ghcr.io/zeroqn/loftd:latest";
const AGENTBOX_GUEST_INIT_ENTRYPOINT: &str = "/bin/agentbox-guest-init";
const CONTAINER_WORKDIR: &str = "/workspace";
const HOST_NIX_UPPER_DIR: &str = "nix-upper";
const HOST_NIX_WORK_DIR: &str = "nix-work";
const HOST_NIX_MERGED_DIR: &str = "nix-merged";
const HOST_NIX_SIDECAR_STATE_FILE: &str = "nix-sidecar.state";
const CONTAINER_CODEX_DIR: &str = "/home/dev/.codex";
const CONTAINER_PI_DIR: &str = "/home/dev/.pi";
const CONTAINER_CARGO_DIR: &str = "/home/dev/.cargo";
const CONTAINER_SCCACHE_DIR: &str = "/home/dev/.cache/sccache";
const CONTAINER_NIX_DIR: &str = "/nix";
const CONTAINER_TMP_TMPFS: &str = "/tmp:rw,exec,mode=1777";
const NIX_STORE_DIR: &str = "store";
const INTERACTIVE_SHELL: &str = "fish";
const NIX_REMOTE_SOCKET: &str = "unix:///nix/var/nix/daemon-socket/socket";
const SIDECAR_SOCKET_PATH: &str = "/nix/var/nix/daemon-socket/socket";
const SIDECAR_HEALTH_ATTEMPTS: u32 = 30;
const SIDECAR_HEALTH_DELAY_MS: u64 = 200;
const SIDECAR_LOG_TAIL_LINES: u32 = 120;
const TASK_CONTAINER_ROLE_LABEL: &str = "io.agentbox.role";
const TASK_CONTAINER_ROLE_VALUE: &str = "task";
const TASK_CONTAINER_SIDECAR_LABEL: &str = "io.agentbox.sidecar";

pub fn entrypoint() -> ExitCode {
    let mut args = std::env::args_os();
    let _program = args.next();
    match args.next().as_deref() {
        Some(arg) if arg == std::ffi::OsStr::new(MICROVM_HELPER_ARG) => {
            let Some(config_path) = args.next() else {
                eprintln!("agentbox: microvm helper requires a launch config path");
                return ExitCode::from(1);
            };
            return match runtime::microvm::run_helper_from_path(std::path::Path::new(&config_path))
            {
                Ok(()) => ExitCode::from(0),
                Err(err) => {
                    eprintln!("agentbox: {err:#}");
                    ExitCode::from(1)
                }
            };
        }
        Some(arg) if arg == std::ffi::OsStr::new("internal") => {
            let Some(command) = args.next() else {
                eprintln!("agentbox: internal command is required");
                return ExitCode::from(1);
            };
            if command != std::ffi::OsStr::new("microvm-ingest-cache") {
                eprintln!(
                    "agentbox: unsupported internal command '{}'",
                    command.to_string_lossy()
                );
                return ExitCode::from(1);
            }
            let Some(image_ref) = args.next() else {
                eprintln!("agentbox: internal microvm-ingest-cache requires an image reference");
                return ExitCode::from(1);
            };
            let Some(cache_root) = args.next() else {
                eprintln!("agentbox: internal microvm-ingest-cache requires a cache root");
                return ExitCode::from(1);
            };
            let Some(expected_digest) = args.next() else {
                eprintln!(
                    "agentbox: internal microvm-ingest-cache requires an expected digest argument"
                );
                return ExitCode::from(1);
            };
            return match runtime::microvm::image_cache::run_ingest_cache_child(
                &image_ref.to_string_lossy(),
                std::path::Path::new(&cache_root),
                &expected_digest.to_string_lossy(),
            ) {
                Ok(digest) => {
                    println!("{}", digest.as_str());
                    ExitCode::from(0)
                }
                Err(err) => {
                    eprintln!("agentbox: {err:#}");
                    ExitCode::from(1)
                }
            };
        }
        _ => {}
    }

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

#[cfg(test)]
mod tests;
