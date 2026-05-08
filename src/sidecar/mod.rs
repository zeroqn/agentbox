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

const SIDECAR_ENTRYPOINT: &str = "/bin/agentbox-nix-sidecar-entrypoint";

use image_mount::{inspect_image_id, mount_image_with_lowerdir, unmount_image};
use overlay::{cleanup_merged_mount, cleanup_merged_mount_all_namespaces, mount_fuse_overlayfs};
pub(crate) use runtime::SidecarNixRuntime;

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
    proxy_port: Option<u16>,
    native_config: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PodmanImageMountMode {
    Direct,
    Unshare,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct SidecarDaemonRuntimeSpec {
    pub socket_health_probe: SidecarSocketHealthProbe,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SidecarSocketHealthProbe {
    Enabled,
    Disabled,
}

impl SidecarSocketHealthProbe {
    fn enabled(self) -> bool {
        self == Self::Enabled
    }
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
    fn matches_identity(&self, image: &str, image_id: &str, sidecar_name: &str) -> bool {
        self.image == image && self.image_id == image_id && self.sidecar_name == sidecar_name
    }

    fn matches(&self, image: &str, image_id: &str, sidecar_name: &str) -> bool {
        self.matches_identity(image, image_id, sidecar_name) && self.native_config
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

pub(crate) fn prepare_sidecar_nix_runtime(
    cwd: &Path,
    state_root: &Path,
    image: &str,
    runtime_spec: SidecarDaemonRuntimeSpec,
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
        let reusable_config_matches = state.matches(image, &image_id, &sidecar_name);
        if should_reuse_previous_sidecar(
            state,
            &paths,
            image,
            reusable_config_matches,
            runtime_spec.socket_health_probe,
        )? {
            let proxy_port = resolve_sidecar_proxy_port(&sidecar_name).unwrap_or_else(|err| {
                eprintln!("agentbox: warning: failed to resolve sidecar proxy port: {err:#}");
                19876
            });
            return Ok(SidecarNixRuntime {
                merged_dir: paths.merged_dir,
                sidecar_name: sidecar_name.clone(),
                proxy_port,
                mount_mode: state.mount_mode,
            });
        }
    }

    reject_active_legacy_sidecar_config(previous_state.as_ref(), image, &image_id, &sidecar_name)?;

    recreate_sidecar_stack(
        &paths,
        image,
        &image_id,
        &sidecar_name,
        previous_state.as_ref(),
        runtime_spec,
    )
}

pub fn cleanup_idle_sidecar(sidecar: &SidecarNixRuntime) -> Result<()> {
    if preserve_idle_sidecar(sidecar_has_running_task_containers(&sidecar.sidecar_name)?) {
        return Ok(());
    }

    cleanup_sidecar_container(&sidecar.sidecar_name)?;
    cleanup_merged_mount(&sidecar.merged_dir, sidecar.mount_mode)
}

fn preserve_idle_sidecar(has_running_task_containers: bool) -> bool {
    has_running_task_containers
}

fn recreate_sidecar_stack(
    paths: &SidecarPaths,
    image: &str,
    image_id: &str,
    sidecar_name: &str,
    previous_state: Option<&SidecarState>,
    runtime_spec: SidecarDaemonRuntimeSpec,
) -> Result<SidecarNixRuntime> {
    if let Some(state) = previous_state {
        cleanup_sidecar_container(&state.sidecar_name)?;
        cleanup_merged_mount(&paths.merged_dir, state.mount_mode)?;
        unmount_image(&state.image)?;
    } else {
        cleanup_sidecar_container(sidecar_name)?;
        cleanup_merged_mount_all_namespaces(&paths.merged_dir)?;
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
    let sidecar_args = build_sidecar_podman_args(image, sidecar_name, &merged_mount_arg)?;
    let status = run_podman(
        sidecar_args,
        Stdio::null(),
        Stdio::null(),
        Stdio::inherit(),
        "failed to start nix-daemon sidecar",
    )?;
    if !status.success() {
        return Err(anyhow!(
            "nix-daemon sidecar '{}' failed to start; ensure image '{}' contains sidecar entrypoint '{}' and rebuild/load the image if needed",
            sidecar_name,
            image,
            SIDECAR_ENTRYPOINT
        ));
    }

    if runtime_spec.socket_health_probe.enabled() {
        health::wait_for_socket_health(image, sidecar_name, &paths.merged_dir, mount_mode)?;
    }

    let proxy_port = resolve_sidecar_proxy_port(sidecar_name).unwrap_or_else(|err| {
        eprintln!("agentbox: warning: failed to resolve sidecar proxy port: {err:#}");
        19876
    });

    let new_state = SidecarState {
        image: image.to_owned(),
        image_id: image_id.to_owned(),
        image_mount_path,
        sidecar_name: sidecar_name.to_owned(),
        mount_mode,
        proxy_port: Some(proxy_port),
        native_config: true,
    };
    state::write_sidecar_state(paths, &new_state)?;

    Ok(SidecarNixRuntime {
        merged_dir: paths.merged_dir.clone(),
        sidecar_name: sidecar_name.to_owned(),
        proxy_port,
        mount_mode,
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
    reusable_config_matches: bool,
    socket_health_probe: SidecarSocketHealthProbe,
) -> Result<bool> {
    if !reusable_config_matches {
        return Ok(false);
    }

    let sidecar_running = health::is_container_running(&state.sidecar_name);
    let protected_same_repo_reuse = protected_same_repo_reuse_applies(
        reusable_config_matches,
        sidecar_running,
        sidecar_has_running_task_containers(&state.sidecar_name),
    );
    if protected_same_repo_reuse {
        return Ok(true);
    }

    Ok(fallback_health_gated_reuse_applies(
        reusable_config_matches,
        protected_same_repo_reuse,
        sidecar_stack_is_reusable(state, paths, image, socket_health_probe)?,
    ))
}

fn sidecar_stack_is_reusable(
    state: &SidecarState,
    paths: &SidecarPaths,
    image: &str,
    socket_health_probe: SidecarSocketHealthProbe,
) -> Result<bool> {
    if socket_health_probe.enabled() {
        health::sidecar_stack_is_healthy(state, paths, image)
    } else {
        health::sidecar_stack_is_present(state, paths)
    }
}

fn reject_active_legacy_sidecar_config(
    state: Option<&SidecarState>,
    image: &str,
    image_id: &str,
    sidecar_name: &str,
) -> Result<()> {
    let Some(state) = state else {
        return Ok(());
    };

    if active_legacy_sidecar_config_applies(
        state,
        image,
        image_id,
        sidecar_name,
        sidecar_has_running_task_containers(&state.sidecar_name)?,
    ) {
        anyhow::bail!(
            "nix-daemon sidecar '{}' was started by a legacy non-container configuration and matching task containers are still active; wait for those tasks to exit before recreating the container-mode sidecar",
            state.sidecar_name
        );
    }

    Ok(())
}

fn active_legacy_sidecar_config_applies(
    state: &SidecarState,
    image: &str,
    image_id: &str,
    sidecar_name: &str,
    running_task_containers: bool,
) -> bool {
    state.matches_identity(image, image_id, sidecar_name)
        && !state.native_config
        && running_task_containers
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

fn build_sidecar_podman_args(
    image: &str,
    sidecar_name: &str,
    merged_mount: &str,
) -> Result<Vec<String>> {
    let mut args = vec![
        "run".to_owned(),
        "-d".to_owned(),
        "--name".to_owned(),
        sidecar_name.to_owned(),
        "--user".to_owned(),
        "0:0".to_owned(),
        "--volume".to_owned(),
        merged_mount.to_owned(),
        "--publish".to_owned(),
        "19876".to_owned(),
    ];

    args.extend([
        "--entrypoint".to_owned(),
        SIDECAR_ENTRYPOINT.to_owned(),
        image.to_owned(),
    ]);

    Ok(args)
}

fn resolve_sidecar_proxy_port(sidecar_name: &str) -> Result<u16> {
    let output = run_podman_output(
        vec![
            "port".to_owned(),
            sidecar_name.to_owned(),
            "19876".to_owned(),
        ],
        "failed to resolve sidecar proxy port",
    )?;
    let port_str = output
        .trim()
        .lines()
        .next()
        .and_then(|line| line.rsplit(':').next())
        .with_context(|| {
            format!(
                "unexpected 'podman port' output for '{}': {:?}",
                sidecar_name, output
            )
        })?;
    port_str
        .parse::<u16>()
        .with_context(|| format!("invalid proxy port number: {port_str}"))
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
