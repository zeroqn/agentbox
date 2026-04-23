mod health;
mod image_mount;
mod name;
mod overlay;
mod runtime;
mod state;

use anyhow::{anyhow, Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use crate::mounts::format::format_mount_arg;
use crate::podman::command::{run_podman, run_podman_output};
use crate::{
    CONTAINER_NIX_DIR, HOST_NIX_MERGED_DIR, HOST_NIX_SIDECAR_STATE_FILE, HOST_NIX_UPPER_DIR,
    HOST_NIX_WORK_DIR, NIX_STORE_DIR, TASK_CONTAINER_ROLE_LABEL, TASK_CONTAINER_ROLE_VALUE,
    TASK_CONTAINER_SIDECAR_LABEL,
};

use image_mount::{inspect_image_id, mount_image_with_lowerdir, unmount_image};
use overlay::{cleanup_merged_mount, mount_fuse_overlayfs};
pub use runtime::SidecarNixRuntime;

#[derive(Debug, Clone)]
struct SidecarPaths {
    upper_dir: PathBuf,
    work_dir: PathBuf,
    merged_dir: PathBuf,
    state_file: PathBuf,
}

#[derive(Debug, Clone)]
struct SidecarState {
    image: String,
    image_id: String,
    image_mount_path: PathBuf,
    sidecar_name: String,
    mount_mode: PodmanImageMountMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PodmanImageMountMode {
    Direct,
    Unshare,
}

impl SidecarPaths {
    fn new(state_root: &Path) -> Self {
        Self {
            upper_dir: state_root.join(HOST_NIX_UPPER_DIR),
            work_dir: state_root.join(HOST_NIX_WORK_DIR),
            merged_dir: state_root.join(HOST_NIX_MERGED_DIR),
            state_file: state_root.join(HOST_NIX_SIDECAR_STATE_FILE),
        }
    }
}

impl SidecarState {
    fn matches(&self, image: &str, image_id: &str, sidecar_name: &str) -> bool {
        self.image == image && self.image_id == image_id && self.sidecar_name == sidecar_name
    }
}

impl PodmanImageMountMode {
    fn label(self) -> &'static str {
        match self {
            Self::Direct => "podman image mount",
            Self::Unshare => "podman unshare podman image mount",
        }
    }
}

pub fn prepare_sidecar_nix_runtime(
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
    let sidecar_name = name::derive_sidecar_name(cwd, &image_id);
    let previous_state = state::read_sidecar_state(&paths)?;

    if let Some(state) = previous_state.as_ref() {
        if should_reuse_previous_sidecar(state, &paths, image, &image_id, &sidecar_name)? {
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

pub fn cleanup_idle_sidecar(sidecar: &SidecarNixRuntime) -> Result<()> {
    if sidecar_has_running_task_containers(&sidecar.sidecar_name)? {
        return Ok(());
    }

    cleanup_sidecar_container(&sidecar.sidecar_name)
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

fn resolve_sidecar_lowerdir(image_mount_path: &Path) -> Result<PathBuf> {
    let nested_nix = image_mount_path.join("nix");
    if nested_nix.is_dir() {
        return Ok(nested_nix);
    }

    let root_store = image_mount_path.join(NIX_STORE_DIR);
    if root_store.is_dir() {
        return Ok(image_mount_path.to_path_buf());
    }

    Err(anyhow!(
        "expected either '{}' or '{}' to exist as directories",
        nested_nix.display(),
        root_store.display()
    ))
}

fn resolve_sidecar_lowerdir_for_mode(
    image_mount_path: &Path,
    mode: PodmanImageMountMode,
) -> Result<PathBuf> {
    if mode == PodmanImageMountMode::Direct {
        return resolve_sidecar_lowerdir(image_mount_path);
    }

    let mount_path = image_mount_path.to_str().with_context(|| {
        format!(
            "image mount path '{}' is not valid UTF-8",
            image_mount_path.display()
        )
    })?;
    let script = "mount_path=\"$1\"\nif [ -d \"$mount_path/nix\" ]; then\n  printf '%s\\n' \"$mount_path/nix\"\nelif [ -d \"$mount_path/store\" ]; then\n  printf '%s\\n' \"$mount_path\"\nelse\n  exit 3\nfi";
    let args = vec![
        "unshare".to_owned(),
        "bash".to_owned(),
        "-lc".to_owned(),
        script.to_owned(),
        "agentbox".to_owned(),
        mount_path.to_owned(),
    ];
    let output = run_podman_output(args, "failed to resolve sidecar lowerdir in podman unshare")?;
    let lowerdir = output.trim();
    if lowerdir.is_empty() {
        return Err(anyhow!(
            "podman unshare lowerdir probe returned empty output for '{}'",
            image_mount_path.display()
        ));
    }

    Ok(PathBuf::from(lowerdir))
}

fn should_reuse_previous_sidecar(
    state: &SidecarState,
    paths: &SidecarPaths,
    image: &str,
    image_id: &str,
    sidecar_name: &str,
) -> Result<bool> {
    let identity_matches = state.matches(image, image_id, sidecar_name);
    if !identity_matches {
        return Ok(false);
    }

    let sidecar_running = health::is_container_running(&state.sidecar_name);
    let protected_same_repo_reuse = protected_same_repo_reuse_applies(
        identity_matches,
        sidecar_running,
        sidecar_has_running_task_containers(&state.sidecar_name),
    );
    if protected_same_repo_reuse {
        return Ok(true);
    }

    Ok(fallback_health_gated_reuse_applies(
        identity_matches,
        protected_same_repo_reuse,
        health::sidecar_stack_is_healthy(state, paths, image)?,
    ))
}

fn protected_same_repo_reuse_applies(
    identity_matches: bool,
    sidecar_running: bool,
    running_task_probe: Result<bool>,
) -> bool {
    if !identity_matches || !sidecar_running {
        return false;
    }

    matches!(running_task_probe, Ok(true))
}

fn fallback_health_gated_reuse_applies(
    identity_matches: bool,
    protected_same_repo_reuse: bool,
    sidecar_stack_is_healthy: bool,
) -> bool {
    !protected_same_repo_reuse && identity_matches && sidecar_stack_is_healthy
}

fn sidecar_has_running_task_containers(sidecar_name: &str) -> Result<bool> {
    let args = build_sidecar_task_probe_args(sidecar_name);
    let output = run_podman_output(
        args,
        "failed to inspect running task containers for sidecar cleanup",
    )?;

    Ok(output.lines().any(|line| !line.trim().is_empty()))
}

fn cleanup_sidecar_container(sidecar_name: &str) -> Result<()> {
    let args = vec!["rm".to_owned(), "-f".to_owned(), sidecar_name.to_owned()];
    let _ = run_podman(
        args,
        Stdio::null(),
        Stdio::null(),
        Stdio::null(),
        "failed to remove stale sidecar container",
    );
    Ok(())
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
