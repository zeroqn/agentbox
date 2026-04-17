use anyhow::{anyhow, Context, Result};
use clap::Parser;
use std::collections::hash_map::DefaultHasher;
use std::env;
use std::fs;
use std::hash::{Hash, Hasher};
use std::os::unix::fs::FileTypeExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::thread;
use std::time::Duration;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PodmanImageMountMode {
    Direct,
    Unshare,
}

impl PodmanImageMountMode {
    fn state_value(self) -> &'static str {
        match self {
            Self::Direct => "direct",
            Self::Unshare => "unshare",
        }
    }

    fn from_state_value(value: &str) -> Option<Self> {
        match value {
            "direct" => Some(Self::Direct),
            "unshare" => Some(Self::Unshare),
            _ => None,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Direct => "podman image mount",
            Self::Unshare => "podman unshare podman image mount",
        }
    }
}

#[derive(Debug, Parser)]
#[command(
    name = "agentbox",
    version,
    about = "Launch a Podman shell with the current directory mounted at /workspace",
    after_help = "Examples:\n  agentbox\n  agentbox --pull-latest\n  agentbox --disable-nix-sidecar\n  agentbox --image ghcr.io/example/agentbox:dev\n  AGENTBOX_IMAGE=ghcr.io/example/agentbox:dev agentbox"
)]
struct Cli {
    #[arg(
        long,
        env = "AGENTBOX_IMAGE",
        help = "Container image to run",
        long_help = "Container image to run. If omitted, agentbox prefers localhost/agentbox:latest and falls back to ghcr.io/zeroqn/agentbox:latest. Can also be set with AGENTBOX_IMAGE."
    )]
    image: Option<String>,

    #[arg(
        long,
        help = "Pull and use ghcr.io/zeroqn/agentbox:latest for this run",
        long_help = "Pull and use ghcr.io/zeroqn/agentbox:latest for this run when --image is not set."
    )]
    pull_latest: bool,

    #[arg(
        long,
        help = "Disable sidecar mode and run with seeded .agentbox/nix mounts",
        long_help = "Disable rootless sidecar mode for this run and use seeded .agentbox/nix bind mounts instead."
    )]
    disable_nix_sidecar: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ImageResolutionStrategy {
    Explicit(String),
    PullLatestGhcr,
    PreferLocalhostFallback,
}

#[derive(Debug, Clone)]
struct PersistentNixRoot {
    store_dir: PathBuf,
    var_nix_dir: PathBuf,
    log_nix_dir: PathBuf,
    marker_file: PathBuf,
}

impl PersistentNixRoot {
    fn new(cwd: &Path) -> Self {
        let root = cwd.join(HOST_OVERLAY_DIR).join(HOST_NIX_ROOT_DIR);
        Self {
            store_dir: root.join(NIX_STORE_DIR),
            var_nix_dir: root.join(NIX_VAR_DIR).join("nix"),
            log_nix_dir: root.join(NIX_VAR_DIR).join(NIX_LOG_DIR).join("nix"),
            marker_file: root.join(NIX_MARKER_FILE),
        }
    }

    fn root_dir(&self) -> &Path {
        self.marker_file.parent().unwrap_or_else(|| Path::new("."))
    }

    fn mounts(&self) -> [(&Path, &str); 3] {
        [
            (self.store_dir.as_path(), HOST_NIX_STORE),
            (self.var_nix_dir.as_path(), HOST_NIX_VAR),
            (self.log_nix_dir.as_path(), HOST_NIX_LOG),
        ]
    }
}

#[derive(Debug, Clone)]
struct SidecarNixRuntime {
    merged_dir: PathBuf,
    sidecar_name: String,
}

#[derive(Debug, Clone)]
struct SidecarPaths {
    upper_dir: PathBuf,
    work_dir: PathBuf,
    merged_dir: PathBuf,
    state_file: PathBuf,
}

impl SidecarPaths {
    fn new(cwd: &Path) -> Self {
        let root = cwd.join(HOST_OVERLAY_DIR);
        Self {
            upper_dir: root.join(HOST_NIX_UPPER_DIR),
            work_dir: root.join(HOST_NIX_WORK_DIR),
            merged_dir: root.join(HOST_NIX_MERGED_DIR),
            state_file: root.join(HOST_NIX_SIDECAR_STATE_FILE),
        }
    }
}

#[derive(Debug, Clone)]
struct SidecarState {
    image: String,
    image_id: String,
    image_mount_path: PathBuf,
    sidecar_name: String,
    mount_mode: PodmanImageMountMode,
}

#[derive(Debug, Clone)]
struct SidecarStartupCleanupOutcome {
    summary: String,
    manual_merged_cleanup_required: bool,
}

#[derive(Debug, Clone, Default)]
struct SidecarStartupDiagnostics {
    sidecar_logs: Option<String>,
    sidecar_logs_error: Option<String>,
    socket_probe_failure: Option<String>,
    sidecar_state: Option<String>,
    host_socket_exists: Option<bool>,
}

impl SidecarState {
    fn matches(&self, image: &str, image_id: &str, sidecar_name: &str) -> bool {
        self.image == image && self.image_id == image_id && self.sidecar_name == sidecar_name
    }
}

#[derive(Debug, Clone)]
enum NixRuntime {
    Seeded(PersistentNixRoot),
    Sidecar(SidecarNixRuntime),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NixRootState {
    Missing,
    Ready,
    Inconsistent,
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

fn resolve_image(cli_image: Option<&str>, pull_latest: bool) -> Result<String> {
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

fn podman_image_exists(image: &str) -> Result<bool> {
    let args = vec!["image".to_owned(), "exists".to_owned(), image.to_owned()];
    let output = run_podman_capture(args, "failed to check whether default image exists")?;
    Ok(output.status.success())
}

fn pull_image(image: &str) -> Result<()> {
    let args = vec!["pull".to_owned(), image.to_owned()];
    let status = run_podman(
        args,
        Stdio::null(),
        Stdio::inherit(),
        Stdio::inherit(),
        "failed to pull container image",
    )?;
    if !status.success() {
        return Err(anyhow!("podman pull '{}' failed", image));
    }

    Ok(())
}

fn build_podman_args(
    image: &str,
    workspace_mount: &str,
    codex_mount: &str,
    cargo_mount: &str,
    nix_runtime: &NixRuntime,
) -> Result<Vec<String>> {
    let mut args = vec![
        "run".to_owned(),
        "--rm".to_owned(),
        "-it".to_owned(),
        "--userns".to_owned(),
        "keep-id".to_owned(),
        "--workdir".to_owned(),
        CONTAINER_WORKDIR.to_owned(),
        "--volume".to_owned(),
        workspace_mount.to_owned(),
        "--volume".to_owned(),
        codex_mount.to_owned(),
        "--volume".to_owned(),
        cargo_mount.to_owned(),
        "--tmpfs".to_owned(),
        CONTAINER_TMP_TMPFS.to_owned(),
    ];

    match nix_runtime {
        NixRuntime::Seeded(persistent_nix_root) => {
            for (source, destination) in persistent_nix_root.mounts() {
                args.push("--volume".to_owned());
                args.push(format_mount_arg(source, destination)?);
            }
        }
        NixRuntime::Sidecar(sidecar) => {
            args.push("--volume".to_owned());
            args.push(format_mount_arg_with_options(
                &sidecar.merged_dir,
                CONTAINER_NIX_DIR,
                Some("ro"),
            )?);
            args.push("--env".to_owned());
            args.push(format!("NIX_REMOTE={NIX_REMOTE_SOCKET}"));
            args.push("--label".to_owned());
            args.push(format!(
                "{TASK_CONTAINER_ROLE_LABEL}={TASK_CONTAINER_ROLE_VALUE}"
            ));
            args.push("--label".to_owned());
            args.push(format!(
                "{TASK_CONTAINER_SIDECAR_LABEL}={}",
                sidecar.sidecar_name
            ));
        }
    }

    args.push(image.to_owned());
    args.push(INTERACTIVE_SHELL.to_owned());
    args.push("-l".to_owned());
    Ok(args)
}

fn resolve_nix_sidecar_enabled(cli: &Cli, env_sidecar_enabled: bool) -> bool {
    if cli.disable_nix_sidecar {
        return false;
    }
    env_sidecar_enabled
}

fn env_flag_enabled(name: &str, default: bool) -> Result<bool> {
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

fn prepare_host_codex_mount() -> Result<String> {
    let home_dir = env::var_os("HOME").context("HOME is not set; cannot locate '~/.codex'")?;
    prepare_host_codex_mount_at(&PathBuf::from(home_dir))
}

fn prepare_host_codex_mount_at(home_dir: &Path) -> Result<String> {
    let codex_dir = home_dir.join(".codex");
    fs::create_dir_all(&codex_dir)
        .with_context(|| format!("failed to create '{}'", codex_dir.display()))?;
    format_mount_arg(&codex_dir, CONTAINER_CODEX_DIR)
}

fn prepare_project_cargo_mount(cwd: &Path) -> Result<String> {
    let cargo_dir = cwd.join(HOST_OVERLAY_DIR).join("cargo");
    fs::create_dir_all(&cargo_dir)
        .with_context(|| format!("failed to create '{}'", cargo_dir.display()))?;
    format_mount_arg(&cargo_dir, CONTAINER_CARGO_DIR)
}

fn prepare_sidecar_nix_runtime(cwd: &Path, image: &str) -> Result<SidecarNixRuntime> {
    // agentbox owns host-side lowerdir/fuse mount lifecycle; the sidecar owns
    // nix-daemon process liveness inside the shared /nix mount.
    ensure_command_available("fuse-overlayfs", "required for sidecar mode")?;

    let paths = SidecarPaths::new(cwd);
    fs::create_dir_all(
        paths
            .upper_dir
            .parent()
            .unwrap_or_else(|| Path::new(HOST_OVERLAY_DIR)),
    )
    .with_context(|| format!("failed to create '{}'", HOST_OVERLAY_DIR))?;
    fs::create_dir_all(&paths.upper_dir)
        .with_context(|| format!("failed to create '{}'", paths.upper_dir.display()))?;
    fs::create_dir_all(&paths.work_dir)
        .with_context(|| format!("failed to create '{}'", paths.work_dir.display()))?;
    fs::create_dir_all(&paths.merged_dir)
        .with_context(|| format!("failed to create '{}'", paths.merged_dir.display()))?;

    let image_id = inspect_image_id(image)?;
    let sidecar_name = derive_sidecar_name(cwd, &image_id);
    let previous_state = read_sidecar_state(&paths)?;

    if let Some(state) = previous_state.as_ref() {
        if state.matches(image, &image_id, &sidecar_name)
            && sidecar_stack_is_healthy(state, &paths, image)?
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

    wait_for_socket_health(image, sidecar_name, &paths.merged_dir)?;

    let new_state = SidecarState {
        image: image.to_owned(),
        image_id: image_id.to_owned(),
        image_mount_path,
        sidecar_name: sidecar_name.to_owned(),
        mount_mode,
    };
    write_sidecar_state(paths, &new_state)?;

    Ok(SidecarNixRuntime {
        merged_dir: paths.merged_dir.clone(),
        sidecar_name: sidecar_name.to_owned(),
    })
}

fn cleanup_idle_sidecar(sidecar: &SidecarNixRuntime) -> Result<()> {
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

fn sidecar_stack_is_healthy(
    state: &SidecarState,
    paths: &SidecarPaths,
    image: &str,
) -> Result<bool> {
    if resolve_sidecar_lowerdir_for_mode(&state.image_mount_path, state.mount_mode).is_err() {
        return Ok(false);
    }

    if !path_is_mounted(&paths.merged_dir)? {
        return Ok(false);
    }

    if !is_container_running(&state.sidecar_name) {
        return Ok(false);
    }

    if daemon_socket_probe_failure(image, &paths.merged_dir)?.is_some() {
        return Ok(false);
    }

    Ok(true)
}

fn is_container_running(container_name: &str) -> bool {
    let args = vec![
        "container".to_owned(),
        "inspect".to_owned(),
        "--format".to_owned(),
        "{{.State.Running}}".to_owned(),
        container_name.to_owned(),
    ];

    match run_podman_output(args, "failed to inspect sidecar container") {
        Ok(output) => output.trim() == "true",
        Err(_) => false,
    }
}

fn daemon_socket_exists(merged_dir: &Path) -> Result<bool> {
    let socket_path = merged_dir
        .join("var")
        .join("nix")
        .join("daemon-socket")
        .join("socket");

    if !socket_path.exists() {
        return Ok(false);
    }

    let metadata = fs::metadata(&socket_path)
        .with_context(|| format!("failed to inspect '{}'", socket_path.display()))?;
    Ok(metadata.file_type().is_socket())
}

fn daemon_socket_probe_failure(image: &str, merged_dir: &Path) -> Result<Option<String>> {
    let merged_mount_arg =
        format_mount_arg_with_options(merged_dir, CONTAINER_NIX_DIR, Some("ro"))?;
    let args = build_socket_ping_podman_args(image, &merged_mount_arg);
    let output = run_podman_capture(args, "failed to probe nix-daemon socket")?;
    if output.status.success() {
        return Ok(None);
    }

    let status = output
        .status
        .code()
        .map_or_else(|| "signal".to_owned(), |code| code.to_string());
    let stderr = summarize_command_output(&output.stderr);
    let stdout = summarize_command_output(&output.stdout);
    let mut details = vec![format!("probe exited with status {status}")];
    if !stderr.is_empty() {
        details.push(format!("stderr: {stderr}"));
    }
    if !stdout.is_empty() {
        details.push(format!("stdout: {stdout}"));
    }
    Ok(Some(details.join("; ")))
}

fn summarize_command_output(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn inspect_sidecar_container_state(sidecar_name: &str) -> Result<String> {
    let args = vec![
        "container".to_owned(),
        "inspect".to_owned(),
        "--format".to_owned(),
        "running={{.State.Running}} status={{.State.Status}} exit_code={{.State.ExitCode}} error={{.State.Error}} oom_killed={{.State.OOMKilled}}".to_owned(),
        sidecar_name.to_owned(),
    ];
    let output = run_podman_output(args, "failed to inspect nix-daemon sidecar state")?;
    let summary = output.trim();
    if summary.is_empty() {
        return Err(anyhow!(
            "nix-daemon sidecar '{}' inspect returned empty output",
            sidecar_name
        ));
    }
    Ok(summary.to_owned())
}

fn wait_for_socket_health(image: &str, sidecar_name: &str, merged_dir: &Path) -> Result<()> {
    let mut last_probe_failure = None;
    let mut last_host_socket_exists = None;
    for _attempt in 0..SIDECAR_HEALTH_ATTEMPTS {
        last_host_socket_exists = Some(daemon_socket_exists(merged_dir)?);
        match daemon_socket_probe_failure(image, merged_dir)? {
            None => return Ok(()),
            Some(probe_failure) => last_probe_failure = Some(probe_failure),
        }
        thread::sleep(Duration::from_millis(SIDECAR_HEALTH_DELAY_MS));
    }

    let (sidecar_logs, sidecar_logs_error) = match read_sidecar_logs(sidecar_name) {
        Ok(logs) => (Some(logs), None),
        Err(err) => (None, Some(err.to_string())),
    };
    let diagnostics = SidecarStartupDiagnostics {
        sidecar_logs,
        sidecar_logs_error,
        socket_probe_failure: last_probe_failure.or_else(|| {
            daemon_socket_probe_failure(image, merged_dir)
                .ok()
                .flatten()
        }),
        sidecar_state: inspect_sidecar_container_state(sidecar_name).ok(),
        host_socket_exists: last_host_socket_exists,
    };
    let cleanup_outcome = cleanup_failed_sidecar_startup(sidecar_name, merged_dir);
    Err(anyhow!(
        "{}",
        build_sidecar_socket_timeout_error(
            sidecar_name,
            merged_dir,
            &cleanup_outcome,
            &diagnostics
        )
    ))
}

fn read_sidecar_logs(sidecar_name: &str) -> Result<String> {
    let args = vec![
        "logs".to_owned(),
        "--tail".to_owned(),
        SIDECAR_LOG_TAIL_LINES.to_string(),
        sidecar_name.to_owned(),
    ];
    let logs = run_podman_output(args, "failed to read nix-daemon sidecar logs")?;
    let trimmed = logs.trim();
    if trimmed.is_empty() {
        return Err(anyhow!(
            "nix-daemon sidecar '{}' emitted no logs",
            sidecar_name
        ));
    }
    Ok(trimmed.to_owned())
}

fn cleanup_failed_sidecar_startup(
    sidecar_name: &str,
    merged_dir: &Path,
) -> SidecarStartupCleanupOutcome {
    let mut summary = Vec::new();

    match cleanup_sidecar_container(sidecar_name) {
        Ok(()) => summary.push(format!(
            "removed sidecar '{}' (or it was already absent)",
            sidecar_name
        )),
        Err(err) => summary.push(format!(
            "failed to remove sidecar '{}': {err:#}",
            sidecar_name
        )),
    }

    let manual_merged_cleanup_required = match cleanup_merged_mount(merged_dir) {
        Ok(()) => {
            summary.push(format!("cleaned merged mount '{}'", merged_dir.display()));
            false
        }
        Err(err) => {
            summary.push(format!(
                "failed to clean merged mount '{}': {err:#}",
                merged_dir.display()
            ));
            true
        }
    };

    SidecarStartupCleanupOutcome {
        summary: summary.join("; "),
        manual_merged_cleanup_required,
    }
}

fn build_sidecar_socket_timeout_error(
    sidecar_name: &str,
    merged_dir: &Path,
    cleanup_outcome: &SidecarStartupCleanupOutcome,
    diagnostics: &SidecarStartupDiagnostics,
) -> String {
    let mut message = format!(
        "nix-daemon socket '{}' was not connectable after startup for sidecar '{}'; {}.",
        SIDECAR_SOCKET_PATH, sidecar_name, cleanup_outcome.summary
    );

    if cleanup_outcome.manual_merged_cleanup_required {
        message.push_str(&format!(
            " The merged mount '{}' could not be cleaned automatically; remove it before retrying.",
            merged_dir.display()
        ));
    } else {
        message.push_str(
            " Automatic cleanup completed; retrying should not require manual '.agentbox/nix-merged' removal.",
        );
    }

    if let Some(logs) = diagnostics
        .sidecar_logs
        .as_deref()
        .map(str::trim)
        .filter(|logs| !logs.is_empty())
    {
        message.push_str("\nrecent sidecar logs:\n");
        message.push_str(logs);
    } else {
        message.push_str("\nsidecar logs unavailable");
        if let Some(err) = diagnostics
            .sidecar_logs_error
            .as_deref()
            .map(str::trim)
            .filter(|err| !err.is_empty())
        {
            message.push_str(&format!(" ({err})"));
        }
        message.push_str(
            "; this usually means the sidecar terminated before logs could be collected.",
        );
    }

    if let Some(state) = diagnostics
        .sidecar_state
        .as_deref()
        .map(str::trim)
        .filter(|state| !state.is_empty())
    {
        message.push_str("\nsidecar state: ");
        message.push_str(state);
    }

    if let Some(probe) = diagnostics
        .socket_probe_failure
        .as_deref()
        .map(str::trim)
        .filter(|probe| !probe.is_empty())
    {
        message.push_str("\nsocket probe failure: ");
        message.push_str(probe);
    }

    if let Some(exists) = diagnostics.host_socket_exists {
        message.push_str("\nhost socket path exists: ");
        message.push_str(if exists { "yes" } else { "no" });
    }

    message
}

fn derive_sidecar_name(cwd: &Path, image_id: &str) -> String {
    let workspace_slug = derive_sidecar_workspace_slug(cwd);
    let mut hasher = DefaultHasher::new();
    cwd.hash(&mut hasher);
    image_id.hash(&mut hasher);
    let digest = hasher.finish();
    format!("{SIDECAR_NAME_PREFIX}-{workspace_slug}-{digest:016x}")
}

fn derive_sidecar_workspace_slug(cwd: &Path) -> String {
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

fn inspect_image_id(image: &str) -> Result<String> {
    let args = vec![
        "image".to_owned(),
        "inspect".to_owned(),
        "--format".to_owned(),
        "{{.Id}}".to_owned(),
        image.to_owned(),
    ];
    let output = run_podman_output(args, "failed to inspect image metadata")?;
    let image_id = output.trim();
    if image_id.is_empty() {
        return Err(anyhow!(
            "podman image inspect returned an empty image ID for '{}'",
            image
        ));
    }
    Ok(image_id.to_owned())
}

fn build_podman_image_mount_args(image: &str, mode: PodmanImageMountMode) -> Vec<String> {
    let args = vec!["image".to_owned(), "mount".to_owned(), image.to_owned()];
    match mode {
        PodmanImageMountMode::Direct => args,
        PodmanImageMountMode::Unshare => build_podman_unshare_args(args),
    }
}

fn build_podman_image_unmount_args(image: &str, mode: PodmanImageMountMode) -> Vec<String> {
    let args = vec!["image".to_owned(), "unmount".to_owned(), image.to_owned()];
    match mode {
        PodmanImageMountMode::Direct => args,
        PodmanImageMountMode::Unshare => build_podman_unshare_args(args),
    }
}

fn mount_image_with_lowerdir(image: &str) -> Result<(PathBuf, PathBuf, PodmanImageMountMode)> {
    let mut attempts = Vec::new();

    for mode in [PodmanImageMountMode::Direct, PodmanImageMountMode::Unshare] {
        match mount_image_once(image, mode) {
            Ok(image_mount_path) => {
                match resolve_sidecar_lowerdir_for_mode(&image_mount_path, mode) {
                    Ok(lowerdir) => return Ok((image_mount_path, lowerdir, mode)),
                    Err(err) => {
                        let _ = unmount_image_mode(image, mode);
                        attempts.push(format!(
                            "{} returned '{}' without a usable lowerdir: {err}",
                            mode.label(),
                            image_mount_path.display(),
                        ));
                    }
                }
            }
            Err(err) => {
                attempts.push(format!("{} failed: {err:#}", mode.label()));
            }
        }
    }

    Err(anyhow!(
        "unable to mount image '{}' with a usable Nix lowerdir; attempts: {}",
        image,
        attempts.join(" | ")
    ))
}

fn mount_image_once(image: &str, mode: PodmanImageMountMode) -> Result<PathBuf> {
    let args = build_podman_image_mount_args(image, mode);
    let output = run_podman_output(args, "failed to mount image rootfs")?;
    let mount_path = output
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .map(str::trim)
        .ok_or_else(|| anyhow!("podman image mount returned no mount path for '{}'", image))?;

    let path = PathBuf::from(mount_path);
    if !path.is_dir() {
        return Err(anyhow!(
            "podman image mount path '{}' is not a directory",
            path.display()
        ));
    }

    Ok(path)
}

fn unmount_image(image: &str) -> Result<()> {
    for mode in [PodmanImageMountMode::Direct, PodmanImageMountMode::Unshare] {
        let _ = unmount_image_mode(image, mode);
    }
    Ok(())
}

fn unmount_image_mode(image: &str, mode: PodmanImageMountMode) -> Result<()> {
    let args = build_podman_image_unmount_args(image, mode);
    let _ = run_podman(
        args,
        Stdio::null(),
        Stdio::null(),
        Stdio::null(),
        "failed to unmount image",
    );
    Ok(())
}

fn mount_fuse_overlayfs(
    lowerdir: &Path,
    upperdir: &Path,
    workdir: &Path,
    merged: &Path,
    mode: PodmanImageMountMode,
) -> Result<()> {
    let overlay_opts = format!(
        "lowerdir={},upperdir={},workdir={}",
        lowerdir.display(),
        upperdir.display(),
        workdir.display()
    );

    let mut command = Command::new("fuse-overlayfs");
    if mode == PodmanImageMountMode::Unshare {
        command = {
            let mut podman_unshare = Command::new("podman");
            podman_unshare.arg("unshare").arg("fuse-overlayfs");
            podman_unshare
        };
    }

    let status = command
        .arg("-o")
        .arg(&overlay_opts)
        .arg(merged)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|err| match err.kind() {
            std::io::ErrorKind::NotFound => {
                anyhow!("fuse-overlayfs is not installed or not available on PATH")
            }
            _ => err.into(),
        })
        .with_context(|| {
            format!(
                "failed to mount fuse-overlayfs with lowerdir='{}' upperdir='{}' workdir='{}'",
                lowerdir.display(),
                upperdir.display(),
                workdir.display()
            )
        })?;

    if !status.success() {
        return Err(anyhow!(
            "fuse-overlayfs mount failed for '{}' (lower='{}', upper='{}', work='{}')",
            merged.display(),
            lowerdir.display(),
            upperdir.display(),
            workdir.display()
        ));
    }

    Ok(())
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
    let script = "mount_path=\"$1\"
if [ -d \"$mount_path/nix\" ]; then
  printf '%s\\n' \"$mount_path/nix\"
elif [ -d \"$mount_path/store\" ]; then
  printf '%s\\n' \"$mount_path\"
else
  exit 3
fi";
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

fn cleanup_merged_mount(merged_dir: &Path) -> Result<()> {
    if !path_is_mounted(merged_dir)? {
        return Ok(());
    }

    for (command, args) in [
        ("fusermount3", vec!["-u"]),
        ("fusermount", vec!["-u"]),
        ("umount", vec![]),
        ("podman", vec!["unshare", "fusermount3", "-u"]),
        ("podman", vec!["unshare", "fusermount", "-u"]),
        ("podman", vec!["unshare", "umount"]),
    ] {
        let mut cmd = Command::new(command);
        for arg in &args {
            cmd.arg(arg);
        }
        let status = cmd
            .arg(merged_dir)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();

        match status {
            Ok(exit_status) if exit_status.success() => return Ok(()),
            Ok(_) => continue,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => continue,
        }
    }

    if path_is_mounted(merged_dir)? {
        return Err(anyhow!(
            "failed to unmount stale fuse mount '{}'; unmount it manually before retrying",
            merged_dir.display()
        ));
    }

    Ok(())
}

fn path_is_mounted(path: &Path) -> Result<bool> {
    if !path.exists() {
        return Ok(false);
    }

    let target = path
        .canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .to_string();

    let mountinfo = fs::read_to_string("/proc/self/mountinfo")
        .context("failed to read /proc/self/mountinfo for mount health check")?;

    for line in mountinfo.lines() {
        let mut fields = line.split_whitespace();
        let _mount_id = fields.next();
        let _parent_id = fields.next();
        let _major_minor = fields.next();
        let _root = fields.next();
        let mount_point = fields.next();

        if mount_point == Some(target.as_str()) {
            return Ok(true);
        }
    }

    Ok(false)
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

fn build_socket_ping_podman_args(image: &str, merged_mount: &str) -> Vec<String> {
    vec![
        "run".to_owned(),
        "--rm".to_owned(),
        "--userns".to_owned(),
        "keep-id".to_owned(),
        "--volume".to_owned(),
        merged_mount.to_owned(),
        image.to_owned(),
        "bash".to_owned(),
        "-lc".to_owned(),
        format!("nix store ping --store {NIX_REMOTE_SOCKET}"),
    ]
}

fn read_sidecar_state(paths: &SidecarPaths) -> Result<Option<SidecarState>> {
    if !paths.state_file.exists() {
        return Ok(None);
    }

    let contents = fs::read_to_string(&paths.state_file)
        .with_context(|| format!("failed to read '{}'", paths.state_file.display()))?;

    match parse_sidecar_state(&contents, &paths.state_file) {
        Ok(state) => Ok(Some(state)),
        Err(err) => {
            match fs::remove_file(&paths.state_file) {
                Ok(()) => {}
                Err(remove_err) if remove_err.kind() == std::io::ErrorKind::NotFound => {}
                Err(remove_err) => {
                    return Err(remove_err).with_context(|| {
                        format!(
                            "failed to remove stale sidecar state '{}' after parse error: {err:#}",
                            paths.state_file.display()
                        )
                    });
                }
            }
            eprintln!(
                "agentbox: ignored stale sidecar state '{}'; recreating sidecar stack ({err:#})",
                paths.state_file.display()
            );
            Ok(None)
        }
    }
}

fn parse_sidecar_state(contents: &str, state_file: &Path) -> Result<SidecarState> {
    let mut image = None;
    let mut image_id = None;
    let mut image_mount_path = None;
    let mut sidecar_name = None;
    let mut mount_mode = None;

    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        if let Some((key, value)) = trimmed.split_once('=') {
            match key {
                "image" => image = Some(value.to_owned()),
                "image_id" => image_id = Some(value.to_owned()),
                "image_mount_path" => image_mount_path = Some(PathBuf::from(value)),
                "sidecar_name" => sidecar_name = Some(value.to_owned()),
                "mount_mode" => {
                    mount_mode = Some(PodmanImageMountMode::from_state_value(value).ok_or_else(
                        || {
                            anyhow!(
                                "unsupported mount_mode '{}' in '{}'",
                                value,
                                state_file.display()
                            )
                        },
                    )?)
                }
                _ => {}
            }
        }
    }

    match (image, image_id, image_mount_path, sidecar_name) {
        (Some(image), Some(image_id), Some(image_mount_path), Some(sidecar_name)) => {
            Ok(SidecarState {
                image,
                image_id,
                image_mount_path,
                sidecar_name,
                mount_mode: mount_mode.unwrap_or(PodmanImageMountMode::Direct),
            })
        }
        _ => Err(anyhow!("'{}' is incomplete", state_file.display())),
    }
}

fn write_sidecar_state(paths: &SidecarPaths, state: &SidecarState) -> Result<()> {
    let parent = paths
        .state_file
        .parent()
        .unwrap_or_else(|| Path::new(HOST_OVERLAY_DIR));
    fs::create_dir_all(parent)
        .with_context(|| format!("failed to create '{}'", parent.display()))?;

    let contents = format!(
        "image={}\nimage_id={}\nimage_mount_path={}\nsidecar_name={}\nmount_mode={}\n",
        state.image,
        state.image_id,
        state.image_mount_path.display(),
        state.sidecar_name,
        state.mount_mode.state_value()
    );

    fs::write(&paths.state_file, contents)
        .with_context(|| format!("failed to write '{}'", paths.state_file.display()))
}

fn ensure_command_available(command: &str, guidance: &str) -> Result<()> {
    let status = Command::new(command)
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

fn prepare_persistent_nix_root(cwd: &Path, image: &str) -> Result<PersistentNixRoot> {
    let nix_root = PersistentNixRoot::new(cwd);
    match inspect_persistent_nix_root(&nix_root)? {
        NixRootState::Ready => {
            ensure_persistent_nix_log_dir(&nix_root)?;
            return Ok(nix_root);
        }
        NixRootState::Inconsistent => {
            return Err(anyhow!(
                "'{}' contains partial Nix state without '{}'; remove or repair '.agentbox/nix' before retrying",
                nix_root.store_dir.parent().unwrap_or(cwd).display(),
                nix_root
                    .marker_file
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
            ));
        }
        NixRootState::Missing => {}
    }

    seed_persistent_nix_root(&nix_root, image, false)?;
    ensure_persistent_nix_log_dir(&nix_root)?;
    fs::write(
        &nix_root.marker_file,
        format!("seeded-from-image={image}\n"),
    )
    .with_context(|| format!("failed to write '{}'", nix_root.marker_file.display()))?;

    Ok(nix_root)
}

fn inspect_persistent_nix_root(nix_root: &PersistentNixRoot) -> Result<NixRootState> {
    let marker_exists = nix_root.marker_file.is_file();
    let store_exists = nix_root.store_dir.is_dir();
    let var_exists = nix_root.var_nix_dir.is_dir();

    if marker_exists {
        return if store_exists && var_exists {
            Ok(NixRootState::Ready)
        } else {
            Ok(NixRootState::Inconsistent)
        };
    }

    let store_empty = dir_is_empty_or_missing(&nix_root.store_dir)?;
    let var_empty = dir_is_empty_or_missing(&nix_root.var_nix_dir)?;
    let log_empty = dir_is_empty_or_missing(&nix_root.log_nix_dir)?;

    if store_empty && var_empty && log_empty {
        Ok(NixRootState::Missing)
    } else {
        Ok(NixRootState::Inconsistent)
    }
}

fn dir_is_empty_or_missing(path: &Path) -> Result<bool> {
    if !path.exists() {
        return Ok(true);
    }
    let mut entries =
        fs::read_dir(path).with_context(|| format!("failed to read '{}'", path.display()))?;
    Ok(entries.next().is_none())
}

fn seed_persistent_nix_root(
    nix_root: &PersistentNixRoot,
    image: &str,
    replace_existing: bool,
) -> Result<()> {
    let root_dir = nix_root.root_dir();
    fs::create_dir_all(root_dir)
        .with_context(|| format!("failed to create '{}'", root_dir.display()))?;

    let host_seed_mount = format_mount_arg(root_dir, SEED_MOUNT_POINT)?;
    let seed_script = build_seed_script(replace_existing);
    let args = build_seed_podman_args(image, &host_seed_mount, &seed_script);

    let status = run_podman(
        args,
        Stdio::null(),
        Stdio::inherit(),
        Stdio::inherit(),
        "failed to seed the project-local Nix root from the container image",
    )?;
    if !status.success() {
        return Err(anyhow!(
            "seeding '.agentbox/nix' from image '{}' failed",
            image
        ));
    }

    Ok(())
}

fn build_seed_podman_args(image: &str, host_seed_mount: &str, seed_script: &str) -> Vec<String> {
    vec![
        "run".to_owned(),
        "--rm".to_owned(),
        "--user".to_owned(),
        "0:0".to_owned(),
        "--volume".to_owned(),
        host_seed_mount.to_owned(),
        image.to_owned(),
        "bash".to_owned(),
        "-lc".to_owned(),
        seed_script.to_owned(),
    ]
}

fn build_seed_script(replace_existing: bool) -> String {
    let mut script = String::from("set -euo pipefail\n");
    if replace_existing {
        script.push_str(&format!(
            "find {mount} -mindepth 1 -maxdepth 1 -exec rm -rf -- {{}} +\n",
            mount = SEED_MOUNT_POINT,
        ));
    }
    script.push_str(&format!(
        "mkdir -p {mount}/{store}\nmkdir -p {mount}/{var_dir}/nix\nmkdir -p {mount}/{var_dir}/{log_dir}/nix\ncp -a {nix_store}/. {mount}/{store}/\nif [ -d {nix_var} ]; then\n  cp -a {nix_var}/. {mount}/{var_dir}/nix/\nfi\n",
        mount = SEED_MOUNT_POINT,
        store = NIX_STORE_DIR,
        var_dir = NIX_VAR_DIR,
        log_dir = NIX_LOG_DIR,
        nix_store = HOST_NIX_STORE,
        nix_var = HOST_NIX_VAR,
    ));
    script
}

fn ensure_persistent_nix_log_dir(nix_root: &PersistentNixRoot) -> Result<()> {
    fs::create_dir_all(&nix_root.log_nix_dir)
        .with_context(|| format!("failed to create '{}'", nix_root.log_nix_dir.display()))
}

fn run_podman(
    args: Vec<String>,
    stdin: Stdio,
    stdout: Stdio,
    stderr: Stdio,
    context: &str,
) -> Result<std::process::ExitStatus> {
    Command::new("podman")
        .args(args)
        .stdin(stdin)
        .stdout(stdout)
        .stderr(stderr)
        .status()
        .map_err(|err| match err.kind() {
            std::io::ErrorKind::NotFound => {
                anyhow!("podman is not installed or not available on PATH")
            }
            _ => err.into(),
        })
        .with_context(|| context.to_owned())
}

fn run_podman_output(args: Vec<String>, context: &str) -> Result<String> {
    let output = Command::new("podman")
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|err| match err.kind() {
            std::io::ErrorKind::NotFound => {
                anyhow!("podman is not installed or not available on PATH")
            }
            _ => err.into(),
        })
        .with_context(|| context.to_owned())?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        if stderr.is_empty() {
            return Err(anyhow!("{}", context));
        }
        return Err(anyhow!("{}: {}", context, stderr));
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn run_podman_capture(args: Vec<String>, context: &str) -> Result<std::process::Output> {
    Command::new("podman")
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|err| match err.kind() {
            std::io::ErrorKind::NotFound => {
                anyhow!("podman is not installed or not available on PATH")
            }
            _ => err.into(),
        })
        .with_context(|| context.to_owned())
}

fn build_podman_unshare_args(mut args: Vec<String>) -> Vec<String> {
    let mut wrapped = Vec::with_capacity(args.len() + 2);
    wrapped.push("unshare".to_owned());
    wrapped.push("podman".to_owned());
    wrapped.append(&mut args);
    wrapped
}

fn format_mount_arg(path: &Path, destination: &str) -> Result<String> {
    format_mount_arg_with_options(path, destination, None)
}

fn format_mount_arg_with_options(
    path: &Path,
    destination: &str,
    options: Option<&str>,
) -> Result<String> {
    let path = path.to_str().with_context(|| {
        format!(
            "path '{}' is not valid UTF-8 and cannot be mounted",
            path.display()
        )
    })?;

    let mut mount = format!("{path}:{destination}");
    if let Some(options) = options {
        if !options.is_empty() {
            mount.push(':');
            mount.push_str(options);
        }
    }

    Ok(mount)
}

#[cfg(test)]
mod tests;
