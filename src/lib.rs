use anyhow::{Context, Result};
use clap::Parser;
use std::env;
use std::path::Path;
use std::process::{ExitCode, Stdio};

mod cli;
mod mounts;
mod nix_root;
mod podman;
mod sidecar;
mod state;

use cli::{
    env_flag_enabled, resolve_image, resolve_nix_sidecar_enabled, resolve_task_kvm_enabled, Cli,
};
use mounts::format::format_mount_arg;
use mounts::{prepare_host_codex_mount, prepare_project_cargo_mount, prepare_shared_sccache_mount};
use nix_root::{prepare_persistent_nix_root, PersistentNixRoot};
use podman::command::run_podman;
use podman::task::{build_podman_args, TaskPodmanSpec};
use sidecar::{cleanup_idle_sidecar, prepare_sidecar_nix_runtime, SidecarNixRuntime};
use state::resolve_state_layout;

const DEFAULT_IMAGE: &str = "localhost/agentbox:latest";
const DEFAULT_FALLBACK_IMAGE: &str = "ghcr.io/zeroqn/agentbox:latest";
const CONTAINER_WORKDIR: &str = "/workspace";
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
const CONTAINER_SCCACHE_DIR: &str = "/home/dev/.cache/sccache";
const CONTAINER_NIX_DIR: &str = "/nix";
const CONTAINER_TMP_TMPFS: &str = "/tmp:rw,exec,mode=1777";
const NIX_STORE_DIR: &str = "store";
const NIX_VAR_DIR: &str = "var";
const NIX_LOG_DIR: &str = "log";
const NIX_MARKER_FILE: &str = ".seeded";
const SEED_MOUNT_POINT: &str = "/agentbox-nix";
const INTERACTIVE_SHELL: &str = "fish";
const NIX_REMOTE_SOCKET: &str = "unix:///nix/var/nix/daemon-socket/socket";
const TASK_KVM_DROP_TO_DEV_ENV: &str = "AGENTBOX_KVM_DROP_TO_DEV=1";
const HOST_UID_ENV_PREFIX: &str = "AGENTBOX_HOST_UID=";
const HOST_GID_ENV_PREFIX: &str = "AGENTBOX_HOST_GID=";
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
const DEFAULT_NIX_SIDECAR_ENABLED: bool = true;
const KVM_NIX_PROXY_PORT: u16 = 19876;
const KVM_NIX_PROXY_PORT_ENV: &str = "AGENTBOX_NIX_PROXY_PORT";
pub(crate) const KVM_NIX_PROXY_GUEST_NIX_REMOTE: &str =
    "unix:///tmp/agentbox-nix-daemon.sock";

#[derive(Debug, Clone)]
enum NixRuntime {
    Seeded(PersistentNixRoot),
    Sidecar(SidecarNixRuntime),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TaskContainerMode {
    Native,
    KvmKrunExperimental,
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
    let state_layout = resolve_state_layout(&cwd)?;
    let task_hostname = derive_task_hostname(&cwd);
    let workspace_mount = format_mount_arg(&cwd, CONTAINER_WORKDIR)?;
    let codex_mount = prepare_host_codex_mount()?;
    let cargo_mount = prepare_project_cargo_mount(state_layout.root_dir())?;
    let sccache_mount = prepare_shared_sccache_mount(&state_layout.sccache_dir())?;

    let env_sidecar_enabled =
        env_flag_enabled("AGENTBOX_NIX_SIDECAR", DEFAULT_NIX_SIDECAR_ENABLED)?;
    let nix_sidecar_enabled = resolve_nix_sidecar_enabled(&cli, env_sidecar_enabled);
    let env_task_kvm_enabled = env_flag_enabled("AGENTBOX_TASK_KVM", false)?;
    let task_mode = resolve_task_mode(resolve_task_kvm_enabled(&cli, env_task_kvm_enabled));
    validate_task_mode(task_mode, nix_sidecar_enabled)?;

    let nix_runtime = if nix_sidecar_enabled {
        NixRuntime::Sidecar(prepare_sidecar_nix_runtime(
            &cwd,
            state_layout.root_dir(),
            &image,
        )?)
    } else {
        NixRuntime::Seeded(prepare_persistent_nix_root(
            state_layout.root_dir(),
            &image,
        )?)
    };

    let mut host_proxy_child: Option<std::process::Child> = None;
    if task_mode == TaskContainerMode::KvmKrunExperimental {
        if let NixRuntime::Sidecar(sidecar) = &nix_runtime {
            let sidecar_socket = sidecar.merged_dir.join("var/nix/daemon-socket/socket");
            let listen =
                format!("TCP-LISTEN:{KVM_NIX_PROXY_PORT},fork,reuseaddr,bind=127.0.0.1");
            let connect = format!("UNIX-CONNECT:{}", sidecar_socket.display());

            match std::process::Command::new("socat")
                .args([&listen, &connect])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
            {
                Ok(child) => host_proxy_child = Some(child),
                Err(err) => {
                    eprintln!(
                        "agentbox: warning: failed to start host nix proxy (is socat installed on the host?): {err:#}"
                    );
                }
            }
        }
    }

    let status = run_podman(
        build_podman_args(TaskPodmanSpec {
            image: &image,
            hostname: &task_hostname,
            workspace_mount: &workspace_mount,
            codex_mount: &codex_mount,
            cargo_mount: &cargo_mount,
            sccache_mount: &sccache_mount,
            nix_runtime: &nix_runtime,
            task_mode,
        })?,
        Stdio::inherit(),
        Stdio::inherit(),
        Stdio::inherit(),
        "failed to start podman",
    )?;

    if let Some(mut child) = host_proxy_child {
        let _ = child.kill();
        let _ = child.wait();
    }

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

fn resolve_task_mode(task_kvm_enabled: bool) -> TaskContainerMode {
    if task_kvm_enabled {
        TaskContainerMode::KvmKrunExperimental
    } else {
        TaskContainerMode::Native
    }
}

fn validate_task_mode(task_mode: TaskContainerMode, nix_sidecar_enabled: bool) -> Result<()> {
    if task_mode == TaskContainerMode::KvmKrunExperimental && !nix_sidecar_enabled {
        anyhow::bail!(
            "--task-kvm requires native nix sidecar mode; remove --disable-nix-sidecar and do not set AGENTBOX_NIX_SIDECAR=0"
        );
    }

    Ok(())
}

fn derive_task_hostname(cwd: &Path) -> String {
    format!("{}-{TASK_HOSTNAME_SUFFIX}", derive_workspace_slug(cwd))
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
