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

const DEFAULT_IMAGE: &str = "localhost/agentbox:latest";
const DEFAULT_FALLBACK_IMAGE: &str = "ghcr.io/zeroqn/agentbox:latest";
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
    if args.next().as_deref() == Some(std::ffi::OsStr::new(MICROVM_HELPER_ARG)) {
        let Some(config_path) = args.next() else {
            eprintln!("agentbox: microvm helper requires a launch config path");
            return ExitCode::from(1);
        };
        return match runtime::microvm::run_helper_from_path(std::path::Path::new(&config_path)) {
            Ok(()) => ExitCode::from(0),
            Err(err) => {
                eprintln!("agentbox: {err:#}");
                ExitCode::from(1)
            }
        };
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
