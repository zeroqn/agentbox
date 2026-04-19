mod health;
mod mount;
mod state;

use anyhow::{anyhow, Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use crate::mounts::format_mount_arg;
use crate::podman::{run_podman, run_podman_output};
use crate::{
    CONTAINER_NIX_DIR, HOST_NIX_MERGED_DIR, HOST_NIX_SIDECAR_STATE_FILE, HOST_NIX_UPPER_DIR,
    HOST_NIX_WORK_DIR, TASK_CONTAINER_ROLE_LABEL, TASK_CONTAINER_ROLE_VALUE,
    TASK_CONTAINER_SIDECAR_LABEL,
};

#[cfg(test)]
use health::{
    build_sidecar_socket_timeout_error, build_socket_ping_podman_args,
    SidecarStartupCleanupOutcome, SidecarStartupDiagnostics,
};
#[cfg(test)]
use mount::{
    build_podman_image_mount_args, build_podman_image_unmount_args, derive_sidecar_name,
    resolve_sidecar_lowerdir, PodmanImageMountMode,
};
use mount::{
    cleanup_merged_mount, cleanup_sidecar_container, inspect_image_id, mount_fuse_overlayfs,
    mount_image_with_lowerdir, unmount_image,
};

#[cfg(test)]
use state::{read_sidecar_state, write_sidecar_state};

#[derive(Debug, Clone)]
pub(super) struct SidecarNixRuntime {
    pub(super) merged_dir: PathBuf,
    pub(super) sidecar_name: String,
}

#[derive(Debug, Clone)]
pub(super) struct SidecarPaths {
    pub(super) upper_dir: PathBuf,
    pub(super) work_dir: PathBuf,
    pub(super) merged_dir: PathBuf,
    pub(super) state_file: PathBuf,
}

impl SidecarPaths {
    pub(super) fn new(state_root: &Path) -> Self {
        Self {
            upper_dir: state_root.join(HOST_NIX_UPPER_DIR),
            work_dir: state_root.join(HOST_NIX_WORK_DIR),
            merged_dir: state_root.join(HOST_NIX_MERGED_DIR),
            state_file: state_root.join(HOST_NIX_SIDECAR_STATE_FILE),
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct SidecarState {
    pub(super) image: String,
    pub(super) image_id: String,
    pub(super) image_mount_path: PathBuf,
    pub(super) sidecar_name: String,
    mount_mode: mount::PodmanImageMountMode,
}

impl SidecarState {
    pub(super) fn matches(&self, image: &str, image_id: &str, sidecar_name: &str) -> bool {
        self.image == image && self.image_id == image_id && self.sidecar_name == sidecar_name
    }
}

pub(super) fn prepare_sidecar_nix_runtime(
    cwd: &Path,
    state_root: &Path,
    image: &str,
) -> Result<SidecarNixRuntime> {
    ensure_command_available("fuse-overlayfs", "required for sidecar mode")?;

    let paths = SidecarPaths::new(state_root);
    fs::create_dir_all(state_root)
        .with_context(|| format!("failed to create '{}'", state_root.display()))?;
    fs::create_dir_all(&paths.upper_dir)
        .with_context(|| format!("failed to create '{}'", paths.upper_dir.display()))?;
    fs::create_dir_all(&paths.work_dir)
        .with_context(|| format!("failed to create '{}'", paths.work_dir.display()))?;
    fs::create_dir_all(&paths.merged_dir)
        .with_context(|| format!("failed to create '{}'", paths.merged_dir.display()))?;

    let image_id = inspect_image_id(image)?;
    let sidecar_name = mount::derive_sidecar_name(cwd, &image_id);
    let previous_state = state::read_sidecar_state(&paths)?;

    if let Some(state) = previous_state.as_ref() {
        if state.matches(image, &image_id, &sidecar_name)
            && health::sidecar_stack_is_healthy(state, &paths, image)?
        {
            return Ok(SidecarNixRuntime {
                merged_dir: paths.merged_dir,
                sidecar_name: sidecar_name.clone(),
            });
        }
    }

    recreate_sidecar_stack(
        &paths,
        image,
        &image_id,
        &sidecar_name,
        previous_state.as_ref(),
    )
}

fn recreate_sidecar_stack(
    paths: &SidecarPaths,
    image: &str,
    image_id: &str,
    sidecar_name: &str,
    previous_state: Option<&SidecarState>,
) -> Result<SidecarNixRuntime> {
    if let Some(state) = previous_state {
        cleanup_sidecar_container(&state.sidecar_name)?;
        cleanup_merged_mount(&paths.merged_dir)?;
        unmount_image(&state.image)?;
    } else {
        cleanup_sidecar_container(sidecar_name)?;
        cleanup_merged_mount(&paths.merged_dir)?;
    }

    let (image_mount_path, lowerdir, mount_mode) = mount_image_with_lowerdir(image)?;

    mount_fuse_overlayfs(
        &lowerdir,
        &paths.upper_dir,
        &paths.work_dir,
        &paths.merged_dir,
        mount_mode,
    )?;

    let merged_mount_arg = format_mount_arg(&paths.merged_dir, CONTAINER_NIX_DIR)?;
    let sidecar_args = build_sidecar_podman_args(image, sidecar_name, &merged_mount_arg);
    let status = run_podman(
        sidecar_args,
        Stdio::null(),
        Stdio::null(),
        Stdio::inherit(),
        "failed to start nix-daemon sidecar",
    )?;
    if !status.success() {
        return Err(anyhow!(
            "nix-daemon sidecar '{}' failed to start",
            sidecar_name
        ));
    }

    health::wait_for_socket_health(image, sidecar_name, &paths.merged_dir)?;

    let new_state = SidecarState {
        image: image.to_owned(),
        image_id: image_id.to_owned(),
        image_mount_path,
        sidecar_name: sidecar_name.to_owned(),
        mount_mode,
    };
    state::write_sidecar_state(paths, &new_state)?;

    Ok(SidecarNixRuntime {
        merged_dir: paths.merged_dir.clone(),
        sidecar_name: sidecar_name.to_owned(),
    })
}

pub(super) fn cleanup_idle_sidecar(sidecar: &SidecarNixRuntime) -> Result<()> {
    if sidecar_has_running_task_containers(&sidecar.sidecar_name)? {
        return Ok(());
    }

    cleanup_sidecar_container(&sidecar.sidecar_name)
}

fn sidecar_has_running_task_containers(sidecar_name: &str) -> Result<bool> {
    let args = build_sidecar_task_probe_args(sidecar_name);
    let output = run_podman_output(
        args,
        "failed to inspect running task containers for sidecar cleanup",
    )?;

    Ok(output.lines().any(|line| !line.trim().is_empty()))
}

fn build_sidecar_task_probe_args(sidecar_name: &str) -> Vec<String> {
    vec![
        "ps".to_owned(),
        "--filter".to_owned(),
        format!("label={TASK_CONTAINER_ROLE_LABEL}={TASK_CONTAINER_ROLE_VALUE}"),
        "--filter".to_owned(),
        format!("label={TASK_CONTAINER_SIDECAR_LABEL}={sidecar_name}"),
        "--format".to_owned(),
        "{{.ID}}".to_owned(),
    ]
}

fn build_sidecar_podman_args(image: &str, sidecar_name: &str, merged_mount: &str) -> Vec<String> {
    vec![
        "run".to_owned(),
        "-d".to_owned(),
        "--name".to_owned(),
        sidecar_name.to_owned(),
        "--user".to_owned(),
        "0:0".to_owned(),
        "--volume".to_owned(),
        merged_mount.to_owned(),
        image.to_owned(),
        "bash".to_owned(),
        "-lc".to_owned(),
        build_sidecar_start_script(),
    ]
}

fn build_sidecar_start_script() -> String {
    [
        "set -euo pipefail",
        "mkdir -p /nix/var/nix/daemon-socket",
        "mkdir -p /nix/var/log/nix",
        "chmod 0755 /nix/var/nix/daemon-socket",
        "echo \"agentbox-sidecar: starting nix-daemon\"",
        "if ! command -v nix-daemon >/dev/null 2>&1; then echo \"agentbox-sidecar: nix-daemon not found on PATH\"; exit 127; fi",
        "nix-daemon --daemon",
        "attempt=0",
        "while [ ! -S /nix/var/nix/daemon-socket/socket ]; do",
        "  attempt=$((attempt + 1))",
        "  if [ \"$attempt\" -ge 300 ]; then",
        "    echo \"agentbox-sidecar: daemon socket not created after 30s\"",
        "    ls -ald /nix/var/nix /nix/var/nix/daemon-socket || true",
        "    ls -al /nix/var/nix/daemon-socket || true",
        "    ps -ef | grep -E '[n]ix-daemon' || true",
        "    exit 1",
        "  fi",
        "  sleep 0.1",
        "done",
        "echo \"agentbox-sidecar: daemon socket ready\"",
        "exec tail -f /dev/null",
    ]
    .join("\n")
}

fn ensure_command_available(command: &str, guidance: &str) -> Result<()> {
    let status = std::process::Command::new(command)
        .arg("--help")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();

    match status {
        Ok(_) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Err(anyhow!(
            "{} is not installed or not available on PATH; {}",
            command,
            guidance
        )),
        Err(err) => Err(err).with_context(|| format!("failed to execute '{}'", command)),
    }
}

#[cfg(test)]
mod tests;
