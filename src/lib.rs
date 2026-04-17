use anyhow::{Context, Result};
use clap::Parser;
use std::env;
use std::process::{ExitCode, Stdio};

mod cli;
mod mounts;
mod nix_root;
mod podman;
mod sidecar;

pub(crate) use cli::{env_flag_enabled, resolve_image, resolve_nix_sidecar_enabled, Cli};
pub(crate) use mounts::{
    format_mount_arg, format_mount_arg_with_options, prepare_host_codex_mount,
    prepare_project_cargo_mount,
};
pub(crate) use nix_root::{prepare_persistent_nix_root, PersistentNixRoot};
pub(crate) use podman::{
    build_podman_args, build_podman_unshare_args, podman_image_exists, pull_image, run_podman,
    run_podman_capture, run_podman_output,
};
pub(crate) use sidecar::{cleanup_idle_sidecar, prepare_sidecar_nix_runtime, SidecarNixRuntime};

#[cfg(test)]
pub(crate) use cli::{
    parse_env_flag_value, resolve_image_strategy, select_default_image, ImageResolutionStrategy,
};
#[cfg(test)]
pub(crate) use mounts::prepare_host_codex_mount_at;
#[cfg(test)]
pub(crate) use nix_root::{
    build_seed_podman_args, build_seed_script, ensure_persistent_nix_log_dir,
    inspect_persistent_nix_root, NixRootState,
};
#[cfg(test)]
pub(crate) use sidecar::{
    build_podman_image_mount_args, build_podman_image_unmount_args, build_sidecar_podman_args,
    build_sidecar_socket_timeout_error, build_sidecar_task_probe_args,
    build_socket_ping_podman_args, derive_sidecar_name, read_sidecar_state,
    resolve_sidecar_lowerdir, write_sidecar_state, PodmanImageMountMode, SidecarPaths,
    SidecarStartupCleanupOutcome, SidecarStartupDiagnostics, SidecarState,
};

const DEFAULT_IMAGE: &str = "localhost/agentbox:latest";
const DEFAULT_FALLBACK_IMAGE: &str = "ghcr.io/zeroqn/agentbox:latest";
const CONTAINER_WORKDIR: &str = "/workspace";
const HOST_OVERLAY_DIR: &str = ".agentbox";
const HOST_NIX_ROOT_DIR: &str = "nix";
const HOST_NIX_STORE: &str = "/nix/store";
const HOST_NIX_VAR: &str = "/nix/var/nix";
const HOST_NIX_LOG: &str = "/nix/var/log/nix";
const HOST_NIX_UPPER_DIR: &str = "nix-upper";
const HOST_NIX_WORK_DIR: &str = "nix-work";
const HOST_NIX_MERGED_DIR: &str = "nix-merged";
const HOST_NIX_SIDECAR_STATE_FILE: &str = "nix-sidecar.state";
const CONTAINER_CODEX_DIR: &str = "/home/dev/.codex";
const CONTAINER_CARGO_DIR: &str = "/home/dev/.cargo";
const CONTAINER_NIX_DIR: &str = "/nix";
const CONTAINER_TMP_TMPFS: &str = "/tmp:rw,exec,mode=1777";
const NIX_STORE_DIR: &str = "store";
const NIX_VAR_DIR: &str = "var";
const NIX_LOG_DIR: &str = "log";
const NIX_MARKER_FILE: &str = ".seeded";
const SEED_MOUNT_POINT: &str = "/agentbox-nix";
const INTERACTIVE_SHELL: &str = "fish";
const NIX_REMOTE_SOCKET: &str = "unix:///nix/var/nix/daemon-socket/socket";
const SIDECAR_NAME_PREFIX: &str = "agentbox-nix-sidecar";
const SIDECAR_NAME_SLUG_FALLBACK: &str = "workspace";
const SIDECAR_NAME_SLUG_MAX_LEN: usize = 32;
const SIDECAR_SOCKET_PATH: &str = "/nix/var/nix/daemon-socket/socket";
const SIDECAR_HEALTH_ATTEMPTS: u32 = 30;
const SIDECAR_HEALTH_DELAY_MS: u64 = 200;
const SIDECAR_LOG_TAIL_LINES: u32 = 120;
const TASK_CONTAINER_ROLE_LABEL: &str = "io.agentbox.role";
const TASK_CONTAINER_ROLE_VALUE: &str = "task";
const TASK_CONTAINER_SIDECAR_LABEL: &str = "io.agentbox.sidecar";
const DEFAULT_NIX_SIDECAR_ENABLED: bool = true;

#[derive(Debug, Clone)]
enum NixRuntime {
    Seeded(PersistentNixRoot),
    Sidecar(SidecarNixRuntime),
}

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
    let cwd = env::current_dir()
        .context("failed to resolve current directory")?
        .canonicalize()
        .context("failed to canonicalize current directory")?;
    let image = resolve_image(cli.image.as_deref(), cli.pull_latest)?;
    let workspace_mount = format_mount_arg(&cwd, CONTAINER_WORKDIR)?;
    let codex_mount = prepare_host_codex_mount()?;
    let cargo_mount = prepare_project_cargo_mount(&cwd)?;

    let env_sidecar_enabled =
        env_flag_enabled("AGENTBOX_NIX_SIDECAR", DEFAULT_NIX_SIDECAR_ENABLED)?;
    let nix_sidecar_enabled = resolve_nix_sidecar_enabled(&cli, env_sidecar_enabled);

    let nix_runtime = if nix_sidecar_enabled {
        NixRuntime::Sidecar(prepare_sidecar_nix_runtime(&cwd, &image)?)
    } else {
        NixRuntime::Seeded(prepare_persistent_nix_root(&cwd, &image)?)
    };

    let status = run_podman(
        build_podman_args(
            &image,
            &workspace_mount,
            &codex_mount,
            &cargo_mount,
            &nix_runtime,
        )?,
        Stdio::inherit(),
        Stdio::inherit(),
        Stdio::inherit(),
        "failed to start podman",
    )?;

    if let NixRuntime::Sidecar(sidecar) = &nix_runtime {
        if let Err(err) = cleanup_idle_sidecar(sidecar) {
            eprintln!(
                "agentbox: warning: failed to cleanup idle sidecar '{}': {err:#}",
                sidecar.sidecar_name
            );
        }
    }

    let code = status.code().unwrap_or(1);
    Ok(ExitCode::from(u8::try_from(code).unwrap_or(1)))
}

#[cfg(test)]
mod tests;
