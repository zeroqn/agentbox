mod health;
mod image_mount;
mod name;
mod overlay;
mod runtime;
mod state;

use anyhow::{anyhow, Context, Result};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::mounts::format::format_mount_arg;
use crate::podman::command::{run_podman, run_podman_output};
use crate::{
    CONTAINER_NIX_DIR, HOST_NIX_SIDECAR_STATE_FILE, NIX_STORE_DIR, TASK_CONTAINER_GENERATION_LABEL,
    TASK_CONTAINER_ROLE_LABEL, TASK_CONTAINER_ROLE_VALUE, TASK_CONTAINER_SIDECAR_LABEL,
};

use image_mount::{inspect_image_id, mount_image_with_lowerdir, unmount_image};
use overlay::{cleanup_merged_mount, mount_fuse_overlayfs};
pub use runtime::SidecarNixRuntime;

const SIDECAR_ROOT_DIR: &str = "nix-sidecar";
const SIDECAR_GENERATIONS_DIR: &str = "generations";
const SIDECAR_CURRENT_POINTER_FILE: &str = "current";
const SIDECAR_LOCK_DIR: &str = "lock";
const SIDECAR_LEASES_DIR: &str = "leases";
const SIDECAR_RECORD_FILE: &str = "record";
const SIDECAR_LOCK_OWNER_FILE: &str = "owner";
const SIDECAR_LEGACY_MIGRATED_SUFFIX: &str = ".migrated";
const SIDECAR_GENERATION_DIR_PREFIX: &str = "gen-";
const SIDECAR_LEGACY_UPPER_DIR: &str = "nix-upper";
const SIDECAR_LEGACY_WORK_DIR: &str = "nix-work";
const SIDECAR_LEGACY_MERGED_DIR: &str = "nix-merged";
const SIDECAR_LOCK_RETRY_ATTEMPTS: u32 = 300;
const SIDECAR_LOCK_RETRY_DELAY_MS: u64 = 100;

#[derive(Debug, Clone)]
pub(crate) struct SidecarPaths {
    state_root: PathBuf,
    sidecar_root: PathBuf,
    generations_dir: PathBuf,
    current_pointer: PathBuf,
    lock_dir: PathBuf,
    legacy_state_file: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SidecarState {
    generation: String,
    image: String,
    image_id: String,
    image_mount_path: PathBuf,
    sidecar_name: String,
    mount_mode: PodmanImageMountMode,
    merged_dir: PathBuf,
    upper_dir: PathBuf,
    work_dir: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PodmanImageMountMode {
    Direct,
    Unshare,
}

struct SidecarLockGuard {
    lock_dir: PathBuf,
}

struct PreparedCandidate {
    lease_file: PathBuf,
    state: SidecarState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CandidateAdoptionDecision {
    UseCurrent,
    UseCandidate,
    RejectCandidate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CurrentUnhealthyPolicy {
    FailFast,
    AllowRecovery,
}

impl SidecarPaths {
    fn new(state_root: &Path) -> Self {
        let sidecar_root = state_root.join(SIDECAR_ROOT_DIR);
        Self {
            state_root: state_root.to_path_buf(),
            generations_dir: sidecar_root.join(SIDECAR_GENERATIONS_DIR),
            current_pointer: sidecar_root.join(SIDECAR_CURRENT_POINTER_FILE),
            lock_dir: sidecar_root.join(SIDECAR_LOCK_DIR),
            legacy_state_file: state_root.join(HOST_NIX_SIDECAR_STATE_FILE),
            sidecar_root,
        }
    }

    fn generation_root_dir(&self, generation: &str) -> PathBuf {
        self.generations_dir.join(generation)
    }

    fn generation_record_file(&self, generation: &str) -> PathBuf {
        self.generation_root_dir(generation)
            .join(SIDECAR_RECORD_FILE)
    }

    fn leases_root_dir(&self) -> PathBuf {
        self.sidecar_root.join(SIDECAR_LEASES_DIR)
    }

    fn generation_lease_dir(&self, generation: &str) -> PathBuf {
        self.leases_root_dir().join(generation)
    }

    fn generation_upper_dir(&self, generation: &str) -> PathBuf {
        self.generation_root_dir(generation).join("upper")
    }

    fn generation_work_dir(&self, generation: &str) -> PathBuf {
        self.generation_root_dir(generation).join("work")
    }

    fn generation_merged_dir(&self, generation: &str) -> PathBuf {
        self.generation_root_dir(generation).join("merged")
    }

    fn legacy_upper_dir(&self) -> PathBuf {
        self.state_root.join(SIDECAR_LEGACY_UPPER_DIR)
    }

    fn legacy_work_dir(&self) -> PathBuf {
        self.state_root.join(SIDECAR_LEGACY_WORK_DIR)
    }

    fn legacy_merged_dir(&self) -> PathBuf {
        self.state_root.join(SIDECAR_LEGACY_MERGED_DIR)
    }

    fn migrated_legacy_state_file(&self) -> PathBuf {
        self.state_root.join(format!(
            "{}{}",
            HOST_NIX_SIDECAR_STATE_FILE, SIDECAR_LEGACY_MIGRATED_SUFFIX
        ))
    }

    fn lock_owner_file(&self) -> PathBuf {
        self.lock_dir.join(SIDECAR_LOCK_OWNER_FILE)
    }
}

impl SidecarState {
    fn matches(&self, image: &str, image_id: &str) -> bool {
        self.image == image && self.image_id == image_id
    }

    fn to_runtime(&self, state_root: &Path, lease_file: PathBuf) -> SidecarNixRuntime {
        SidecarNixRuntime {
            lease_file,
            state_root: state_root.to_path_buf(),
            generation: self.generation.clone(),
            merged_dir: self.merged_dir.clone(),
            sidecar_name: self.sidecar_name.clone(),
        }
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

impl Drop for SidecarLockGuard {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.lock_dir);
    }
}

pub fn prepare_sidecar_nix_runtime(
    cwd: &Path,
    state_root: &Path,
    image: &str,
) -> Result<SidecarNixRuntime> {
    ensure_command_available("fuse-overlayfs", "required for sidecar mode")?;

    let paths = SidecarPaths::new(state_root);
    fs::create_dir_all(&paths.sidecar_root)
        .with_context(|| format!("failed to create '{}'", paths.sidecar_root.display()))?;
    fs::create_dir_all(&paths.generations_dir)
        .with_context(|| format!("failed to create '{}'", paths.generations_dir.display()))?;

    let image_id = inspect_image_id(image)?;
    {
        let _lock = acquire_sidecar_lock(&paths)?;
        state::migrate_legacy_state_if_needed(&paths)?;

        if let Some(current_state) = state::read_current_sidecar_state(&paths)? {
            if current_state.matches(image, &image_id) {
                if health::sidecar_stack_is_healthy(&current_state, image)? {
                    let lease_file = register_generation_lease(&paths, &current_state.generation)?;
                    return Ok(current_state.to_runtime(state_root, lease_file));
                }

                let live_references = generation_has_live_references(
                    &paths,
                    &current_state.generation,
                    &current_state.sidecar_name,
                )?;
                let diagnostics = health::build_current_generation_unhealthy_error(
                    &current_state,
                    image,
                    live_references,
                );
                match decide_current_unhealthy_policy(live_references) {
                    CurrentUnhealthyPolicy::FailFast => return Err(anyhow!(diagnostics)),
                    CurrentUnhealthyPolicy::AllowRecovery => {
                        eprintln!("agentbox: warning: {diagnostics}");
                    }
                }
            }
        }
    }

    let candidate = {
        let _lock = acquire_sidecar_lock(&paths)?;
        state::migrate_legacy_state_if_needed(&paths)?;
        create_candidate_generation(&paths, cwd, image, &image_id)?
    };

    if let Err(err) = health::wait_for_socket_health(
        image,
        &candidate.state.sidecar_name,
        &candidate.state.merged_dir,
    ) {
        let _lock = acquire_sidecar_lock(&paths)?;
        let _ = release_generation_lease(&candidate.lease_file);
        let cleanup_err = cleanup_generation_artifacts(&paths, &candidate.state);
        if let Err(cleanup_err) = cleanup_err {
            eprintln!(
                "agentbox: warning: failed to cleanup candidate generation '{}' after startup failure: {cleanup_err:#}",
                candidate.state.generation
            );
        }
        return Err(err);
    }

    let adopted_runtime = {
        let _lock = acquire_sidecar_lock(&paths)?;
        state::migrate_legacy_state_if_needed(&paths)?;
        let candidate_healthy = health::sidecar_stack_is_healthy(&candidate.state, image)?;

        let competing_current =
            if let Some(current_state) = state::read_current_sidecar_state(&paths)? {
                if current_state.matches(image, &image_id)
                    && current_state.generation != candidate.state.generation
                    && health::sidecar_stack_is_healthy(&current_state, image)?
                {
                    Some(current_state)
                } else {
                    None
                }
            } else {
                None
            };

        match decide_candidate_adoption(competing_current.is_some(), candidate_healthy) {
            CandidateAdoptionDecision::UseCurrent => {
                let adopted_state = competing_current.expect("current state should exist");
                let _ = release_generation_lease(&candidate.lease_file);
                if let Err(err) = cleanup_generation_artifacts(&paths, &candidate.state) {
                    eprintln!(
                        "agentbox: warning: failed to discard losing candidate generation '{}': {err:#}",
                        candidate.state.generation
                    );
                }
                let lease_file = register_generation_lease(&paths, &adopted_state.generation)?;
                adopted_state.to_runtime(state_root, lease_file)
            }
            CandidateAdoptionDecision::UseCandidate => {
                state::write_current_generation(&paths, &candidate.state.generation)?;
                finish_post_publish_prune(prune_unused_generations(
                    &paths,
                    Some(&candidate.state.generation),
                ));
                candidate
                    .state
                    .to_runtime(state_root, candidate.lease_file.clone())
            }
            CandidateAdoptionDecision::RejectCandidate => {
                let _ = release_generation_lease(&candidate.lease_file);
                cleanup_generation_artifacts(&paths, &candidate.state)?;
                return Err(anyhow!(
                    "candidate generation '{}' became unhealthy before publish",
                    candidate.state.generation
                ));
            }
        }
    };

    Ok(adopted_runtime)
}

pub fn cleanup_idle_sidecar(sidecar: &SidecarNixRuntime) -> Result<()> {
    let paths = SidecarPaths::new(&sidecar.state_root);
    let _lock = acquire_sidecar_lock(&paths)?;
    state::migrate_legacy_state_if_needed(&paths)?;
    release_generation_lease(&sidecar.lease_file)?;

    if generation_has_live_references(&paths, &sidecar.generation, &sidecar.sidecar_name)? {
        return Ok(());
    }

    let state = match state::read_generation_record(&paths, &sidecar.generation)? {
        Some(state) => state,
        None => return Ok(()),
    };
    cleanup_generation_artifacts(&paths, &state)?;
    state::clear_current_generation(&paths, &sidecar.generation)?;
    prune_unused_generations(&paths, None)
}

fn create_candidate_generation(
    paths: &SidecarPaths,
    cwd: &Path,
    image: &str,
    image_id: &str,
) -> Result<PreparedCandidate> {
    let generation = format!(
        "{SIDECAR_GENERATION_DIR_PREFIX}{}",
        name::allocate_generation_id(cwd, image_id)
    );
    let generation_root = paths.generation_root_dir(&generation);
    let upper_dir = paths.generation_upper_dir(&generation);
    let work_dir = paths.generation_work_dir(&generation);
    let merged_dir = paths.generation_merged_dir(&generation);

    fs::create_dir_all(&generation_root)
        .with_context(|| format!("failed to create '{}'", generation_root.display()))?;
    fs::create_dir_all(&upper_dir)
        .with_context(|| format!("failed to create '{}'", upper_dir.display()))?;
    fs::create_dir_all(&work_dir)
        .with_context(|| format!("failed to create '{}'", work_dir.display()))?;
    fs::create_dir_all(&merged_dir)
        .with_context(|| format!("failed to create '{}'", merged_dir.display()))?;

    let sidecar_name = name::derive_sidecar_name(cwd, image_id, &generation);
    let (image_mount_path, lowerdir, mount_mode) = match mount_image_with_lowerdir(image) {
        Ok(result) => result,
        Err(err) => {
            cleanup_partial_candidate_dirs(&generation_root, &merged_dir, &upper_dir, &work_dir)?;
            return Err(err);
        }
    };
    let state = SidecarState {
        generation,
        image: image.to_owned(),
        image_id: image_id.to_owned(),
        image_mount_path,
        sidecar_name,
        mount_mode,
        merged_dir,
        upper_dir,
        work_dir,
    };
    if let Err(err) = mount_fuse_overlayfs(
        &lowerdir,
        &state.upper_dir,
        &state.work_dir,
        &state.merged_dir,
        mount_mode,
    ) {
        cleanup_generation_artifacts(paths, &state)?;
        return Err(err);
    }

    let merged_mount_arg = format_mount_arg(&state.merged_dir, CONTAINER_NIX_DIR)?;
    let sidecar_args = build_sidecar_podman_args(image, &state.sidecar_name, &merged_mount_arg);
    let status = match run_podman(
        sidecar_args,
        Stdio::null(),
        Stdio::null(),
        Stdio::inherit(),
        "failed to start nix-daemon sidecar",
    ) {
        Ok(status) => status,
        Err(err) => {
            cleanup_generation_artifacts(paths, &state)?;
            return Err(err);
        }
    };
    if !status.success() {
        cleanup_generation_artifacts(paths, &state)?;
        return Err(anyhow!(
            "nix-daemon sidecar '{}' failed to start",
            state.sidecar_name
        ));
    }

    if let Err(err) = state::write_generation_record(paths, &state) {
        cleanup_generation_artifacts(paths, &state)?;
        return Err(err);
    }

    let lease_file = match register_generation_lease(paths, &state.generation) {
        Ok(lease_file) => lease_file,
        Err(err) => {
            cleanup_generation_artifacts(paths, &state)?;
            return Err(err);
        }
    };

    Ok(PreparedCandidate { lease_file, state })
}

fn prune_unused_generations(
    paths: &SidecarPaths,
    protected_generation: Option<&str>,
) -> Result<()> {
    let current_generation = state::read_current_generation(paths)?;
    for generation_state in state::list_generation_records(paths)? {
        if protected_generation == Some(generation_state.generation.as_str()) {
            continue;
        }
        if current_generation.as_deref() == Some(generation_state.generation.as_str()) {
            continue;
        }
        if generation_has_live_references(
            paths,
            &generation_state.generation,
            &generation_state.sidecar_name,
        )? {
            continue;
        }
        cleanup_generation_artifacts(paths, &generation_state)?;
    }
    Ok(())
}

fn cleanup_generation_artifacts(paths: &SidecarPaths, state: &SidecarState) -> Result<()> {
    cleanup_sidecar_container(&state.sidecar_name)?;
    cleanup_merged_mount(&state.merged_dir)?;
    if !other_generation_uses_same_image(paths, state)? {
        unmount_image(&state.image_id)?;
    }

    for dir in [
        &state.merged_dir,
        &state.work_dir,
        &state.upper_dir,
        &paths.generation_root_dir(&state.generation),
    ] {
        match fs::remove_dir_all(dir) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => {
                return Err(err).with_context(|| format!("failed to remove '{}'", dir.display()))
            }
        }
    }

    state::remove_generation_record(paths, &state.generation)?;
    Ok(())
}

fn cleanup_partial_candidate_dirs(
    generation_root: &Path,
    merged_dir: &Path,
    upper_dir: &Path,
    work_dir: &Path,
) -> Result<()> {
    for dir in [merged_dir, work_dir, upper_dir, generation_root] {
        match fs::remove_dir_all(dir) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => {
                return Err(err).with_context(|| format!("failed to remove '{}'", dir.display()))
            }
        }
    }
    Ok(())
}

fn decide_candidate_adoption(
    competing_current_exists: bool,
    candidate_healthy: bool,
) -> CandidateAdoptionDecision {
    if competing_current_exists {
        CandidateAdoptionDecision::UseCurrent
    } else if candidate_healthy {
        CandidateAdoptionDecision::UseCandidate
    } else {
        CandidateAdoptionDecision::RejectCandidate
    }
}

fn decide_current_unhealthy_policy(live_references: bool) -> CurrentUnhealthyPolicy {
    if live_references {
        CurrentUnhealthyPolicy::FailFast
    } else {
        CurrentUnhealthyPolicy::AllowRecovery
    }
}

fn finish_post_publish_prune(prune_result: Result<()>) {
    if let Err(err) = prune_result {
        eprintln!(
            "agentbox: warning: failed to prune stale sidecar generations after publish: {err:#}"
        );
    }
}

fn other_generation_uses_same_image(paths: &SidecarPaths, state: &SidecarState) -> Result<bool> {
    Ok(state::list_generation_records(paths)?
        .into_iter()
        .any(|record| {
            record.generation != state.generation
                && record.image == state.image
                && record.image_id == state.image_id
        }))
}

fn register_generation_lease(paths: &SidecarPaths, generation: &str) -> Result<PathBuf> {
    let lease_dir = paths.generation_lease_dir(generation);
    fs::create_dir_all(&lease_dir)
        .with_context(|| format!("failed to create '{}'", lease_dir.display()))?;

    let lease_file = lease_dir.join(format!(
        "{}-{}.lease",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default()
    ));
    fs::write(&lease_file, format!("pid={}\n", std::process::id()))
        .with_context(|| format!("failed to write '{}'", lease_file.display()))?;
    Ok(lease_file)
}

fn release_generation_lease(lease_file: &Path) -> Result<()> {
    match fs::remove_file(lease_file) {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => {
            return Err(err).with_context(|| format!("failed to remove '{}'", lease_file.display()))
        }
    }

    if let Some(parent) = lease_file.parent() {
        match fs::remove_dir(parent) {
            Ok(()) => {}
            Err(err)
                if err.kind() == std::io::ErrorKind::NotFound
                    || err.kind() == std::io::ErrorKind::DirectoryNotEmpty => {}
            Err(err) => {
                return Err(err).with_context(|| format!("failed to remove '{}'", parent.display()))
            }
        }
    }

    Ok(())
}

fn generation_has_live_references(
    paths: &SidecarPaths,
    generation: &str,
    sidecar_name: &str,
) -> Result<bool> {
    if generation_has_running_task_containers(generation, sidecar_name)? {
        return Ok(true);
    }

    generation_has_live_leases(paths, generation)
}

fn generation_has_live_leases(paths: &SidecarPaths, generation: &str) -> Result<bool> {
    let lease_dir = paths.generation_lease_dir(generation);
    if !lease_dir.exists() {
        return Ok(false);
    }

    for entry in fs::read_dir(&lease_dir)
        .with_context(|| format!("failed to read '{}'", lease_dir.display()))?
    {
        let entry =
            entry.with_context(|| format!("failed to inspect '{}'", lease_dir.display()))?;
        let path = entry.path();
        let contents = match fs::read_to_string(&path) {
            Ok(contents) => contents,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => return Ok(true),
        };
        let Some(pid_line) = contents.lines().find(|line| line.starts_with("pid=")) else {
            return Ok(true);
        };
        let Ok(pid) = pid_line.trim_start_matches("pid=").parse::<u32>() else {
            return Ok(true);
        };
        if Path::new("/proc").join(pid.to_string()).exists() {
            return Ok(true);
        }
        let _ = fs::remove_file(&path);
    }

    let _ = fs::remove_dir(&lease_dir);
    Ok(false)
}

pub(crate) fn resolve_sidecar_lowerdir(image_mount_path: &Path) -> Result<PathBuf> {
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

pub(crate) fn resolve_sidecar_lowerdir_for_mode(
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

pub(crate) fn cleanup_sidecar_container(sidecar_name: &str) -> Result<()> {
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

fn generation_has_running_task_containers(generation: &str, sidecar_name: &str) -> Result<bool> {
    if task_probe_has_running_containers(build_task_probe_args(
        TASK_CONTAINER_GENERATION_LABEL,
        generation,
    ))? {
        return Ok(true);
    }

    task_probe_has_running_containers(build_task_probe_args(
        TASK_CONTAINER_SIDECAR_LABEL,
        sidecar_name,
    ))
}

fn task_probe_has_running_containers(args: Vec<String>) -> Result<bool> {
    let output = run_podman_output(
        args,
        "failed to inspect running task containers for sidecar cleanup",
    )?;

    Ok(output.lines().any(|line| !line.trim().is_empty()))
}

#[allow(dead_code)]
fn build_sidecar_task_probe_args(generation: &str) -> Vec<String> {
    build_task_probe_args(TASK_CONTAINER_GENERATION_LABEL, generation)
}

fn build_task_probe_args(label_key: &str, label_value: &str) -> Vec<String> {
    vec![
        "ps".to_owned(),
        "--filter".to_owned(),
        format!("label={TASK_CONTAINER_ROLE_LABEL}={TASK_CONTAINER_ROLE_VALUE}"),
        "--filter".to_owned(),
        format!("label={label_key}={label_value}"),
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
        "rm -f /nix/var/nix/daemon-socket/socket",
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

fn acquire_sidecar_lock(paths: &SidecarPaths) -> Result<SidecarLockGuard> {
    fs::create_dir_all(&paths.sidecar_root)
        .with_context(|| format!("failed to create '{}'", paths.sidecar_root.display()))?;

    for _attempt in 0..SIDECAR_LOCK_RETRY_ATTEMPTS {
        match fs::create_dir(&paths.lock_dir) {
            Ok(()) => {
                write_lock_owner(paths)?;
                return Ok(SidecarLockGuard {
                    lock_dir: paths.lock_dir.clone(),
                });
            }
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                match lock_owner_status(paths)? {
                    LockOwnerStatus::Dead => match fs::remove_dir_all(&paths.lock_dir) {
                        Ok(()) => continue,
                        Err(remove_err) if remove_err.kind() == std::io::ErrorKind::NotFound => {
                            continue
                        }
                        Err(remove_err) => {
                            return Err(remove_err).with_context(|| {
                                format!(
                                    "failed to remove stale lock '{}'",
                                    paths.lock_dir.display()
                                )
                            })
                        }
                    },
                    LockOwnerStatus::Alive | LockOwnerStatus::Unknown => {}
                }
                thread::sleep(Duration::from_millis(SIDECAR_LOCK_RETRY_DELAY_MS));
            }
            Err(err) => {
                return Err(err)
                    .with_context(|| format!("failed to create '{}'", paths.lock_dir.display()))
            }
        }
    }

    Err(anyhow!(
        "timed out acquiring sidecar workspace lock '{}'",
        paths.lock_dir.display()
    ))
}

fn write_lock_owner(paths: &SidecarPaths) -> Result<()> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default();
    let hostname = env::var("HOSTNAME").unwrap_or_else(|_| "unknown".to_owned());
    let owner = format!(
        "pid={}\ntimestamp={}\nhostname={}\n",
        std::process::id(),
        timestamp,
        hostname
    );
    fs::write(paths.lock_owner_file(), owner).with_context(|| {
        format!(
            "failed to write sidecar lock owner '{}'",
            paths.lock_owner_file().display()
        )
    })
}

enum LockOwnerStatus {
    Alive,
    Dead,
    Unknown,
}

fn lock_owner_status(paths: &SidecarPaths) -> Result<LockOwnerStatus> {
    let owner_file = paths.lock_owner_file();
    let contents = match fs::read_to_string(&owner_file) {
        Ok(contents) => contents,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(LockOwnerStatus::Unknown)
        }
        Err(err) => {
            return Err(err).with_context(|| format!("failed to read '{}'", owner_file.display()))
        }
    };

    let Some(pid_line) = contents.lines().find(|line| line.starts_with("pid=")) else {
        return Ok(LockOwnerStatus::Unknown);
    };
    let Ok(pid) = pid_line.trim_start_matches("pid=").parse::<u32>() else {
        return Ok(LockOwnerStatus::Unknown);
    };
    if Path::new("/proc").join(pid.to_string()).exists() {
        Ok(LockOwnerStatus::Alive)
    } else {
        Ok(LockOwnerStatus::Dead)
    }
}

#[cfg(test)]
mod tests;
