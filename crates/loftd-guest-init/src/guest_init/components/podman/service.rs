use anyhow::{Context, Result, anyhow, bail};
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::guest_init::command;
use crate::guest_init::components::env::{DEV_USER, PODMAN_LOG_PATH};
use crate::guest_init::components::home::identity::DevIdentity;
use crate::guest_init::components::rootless::idmap::WRAPPER_BIN_DIR;
use crate::guest_init::{fs as guest_fs, process};

pub(in crate::guest_init) const REAL_PODMAN_ENV: &str = "LOFTD_REAL_PODMAN";
const SERVICE_LOCK_FILE: &str = "service.lock";
const SOCKET_SUBDIR: &str = "podman";
const SOCKET_FILE: &str = "podman.sock";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::guest_init) struct PodmanServicePaths {
    pub(in crate::guest_init) runtime_dir: PathBuf,
    pub(in crate::guest_init) socket_dir: PathBuf,
    pub(in crate::guest_init) socket_path: PathBuf,
    pub(in crate::guest_init) socket_uri: String,
    lock_path: PathBuf,
}

impl PodmanServicePaths {
    pub(in crate::guest_init) fn for_identity(identity: &DevIdentity) -> Self {
        let runtime_dir = PathBuf::from(format!("/run/user/{}", identity.uid));
        Self::from_runtime_dir(runtime_dir)
    }

    fn from_runtime_dir(runtime_dir: PathBuf) -> Self {
        let socket_dir = runtime_dir.join(SOCKET_SUBDIR);
        let socket_path = socket_dir.join(SOCKET_FILE);
        let socket_uri = format!("unix://{}", socket_path.display());
        let lock_path = socket_dir.join(SERVICE_LOCK_FILE);
        Self {
            runtime_dir,
            socket_dir,
            socket_path,
            socket_uri,
            lock_path,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::guest_init) struct PodmanServiceCommandPlan {
    pub(in crate::guest_init) program: PathBuf,
    pub(in crate::guest_init) args: Vec<String>,
    pub(in crate::guest_init) env: Vec<(String, String)>,
}

pub(in crate::guest_init) fn docker_host_uri(identity: &DevIdentity) -> String {
    PodmanServicePaths::for_identity(identity).socket_uri
}

const ROOTLESS_INFO_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_ROOTLESS_INFO_STDERR: usize = 64 * 1024;

pub(in crate::guest_init) fn verify_rootless_info(identity: &DevIdentity) -> Result<()> {
    let env = PodmanServiceEnv::from_process();
    verify_rootless_info_with_env_and_timeout(identity, &env, ROOTLESS_INFO_TIMEOUT)
}

fn verify_rootless_info_with_env_and_timeout(
    identity: &DevIdentity,
    env: &PodmanServiceEnv,
    timeout: Duration,
) -> Result<()> {
    let paths = PodmanServicePaths::for_identity(identity);
    let program = resolve_real_podman_with_env(env)?;
    let mut command = Command::new(&program);
    command
        .arg("info")
        .env_clear()
        .envs(service_environment_with_env(identity, &paths, env))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    let uid = identity.uid;
    let gid = identity.gid;
    let starts_as_root = process::is_root();
    unsafe {
        command.pre_exec(move || {
            if starts_as_root {
                if libc::setgroups(0, std::ptr::null()) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                if libc::setgid(gid) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                if libc::setuid(uid) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
            }
            Ok(())
        });
    }
    let mut child = command
        .spawn()
        .with_context(|| format!("failed to run {} info as {DEV_USER}", program.display()))?;
    let mut stderr = child.stderr.take().expect("Podman info stderr is piped");
    let stderr_flags = unsafe { libc::fcntl(stderr.as_raw_fd(), libc::F_GETFL) };
    if stderr_flags == -1
        || unsafe {
            libc::fcntl(
                stderr.as_raw_fd(),
                libc::F_SETFL,
                stderr_flags | libc::O_NONBLOCK,
            )
        } == -1
    {
        let err = std::io::Error::last_os_error();
        let _ = child.kill();
        let _ = child.wait();
        return Err(err).context("failed to make Podman info stderr nonblocking");
    }
    let deadline = Instant::now() + timeout;
    let mut stderr_output = Vec::new();
    let status = loop {
        if let Err(err) = drain_nonblocking(&mut stderr, &mut stderr_output) {
            let _ = terminate_and_reap(&mut child);
            return Err(err)
                .with_context(|| format!("failed to read {} info stderr", program.display()));
        }
        let child_status = match child.try_wait() {
            Ok(status) => status,
            Err(err) => {
                let _ = terminate_and_reap(&mut child);
                return Err(err).with_context(|| {
                    format!(
                        "failed to wait for {} info as {DEV_USER}",
                        program.display()
                    )
                });
            }
        };
        if let Some(status) = child_status {
            break status;
        }
        if Instant::now() >= deadline {
            terminate_and_reap(&mut child).with_context(|| {
                format!("failed to terminate timed-out {} info", program.display())
            })?;
            bail!("rootless Podman info timed out after {timeout:?}");
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    drain_nonblocking(&mut stderr, &mut stderr_output)
        .with_context(|| format!("failed to read {} info stderr", program.display()))?;
    let stderr = String::from_utf8_lossy(&stderr_output);
    if status.success() {
        return Ok(());
    }
    let stderr = stderr.trim();
    let detail = if stderr.is_empty() {
        String::new()
    } else {
        format!(": {stderr}")
    };
    bail!("rootless Podman info failed with status {status}{detail}")
}

fn terminate_and_reap(child: &mut std::process::Child) -> std::io::Result<()> {
    let kill_result = child.kill();
    let wait_result = child.wait();
    if let Err(err) = kill_result
        && err.raw_os_error() != Some(libc::ESRCH)
    {
        return Err(err);
    }
    wait_result.map(|_| ())
}

fn drain_nonblocking(reader: &mut impl Read, output: &mut Vec<u8>) -> std::io::Result<()> {
    let mut buffer = [0; 8192];
    for _ in 0..8 {
        match reader.read(&mut buffer) {
            Ok(0) => return Ok(()),
            Ok(read) => {
                let retained = MAX_ROOTLESS_INFO_STDERR.saturating_sub(output.len());
                output.extend_from_slice(&buffer[..read.min(retained)]);
            }
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => return Ok(()),
            Err(err) => return Err(err),
        }
    }
    Ok(())
}

pub(in crate::guest_init) fn ensure_started(
    identity: &DevIdentity,
    timeout: Duration,
) -> Result<()> {
    let paths = PodmanServicePaths::for_identity(identity);
    ensure_started_at(identity, &paths, timeout)
}

fn ensure_started_at(
    identity: &DevIdentity,
    paths: &PodmanServicePaths,
    timeout: Duration,
) -> Result<()> {
    let deadline = Instant::now() + timeout;
    prepare_socket_dir(identity, paths)?;
    if socket_is_live(&paths.socket_path) {
        return Ok(());
    }

    let _lock = PodmanServiceLock::acquire_until(identity, paths, deadline)?;
    if socket_is_live(&paths.socket_path) {
        return Ok(());
    }
    remove_stale_socket(&paths.socket_path)?;
    let plan = command_plan(identity, paths)?;
    spawn_service(identity, &plan)?;
    wait_for_socket(&paths.socket_path, remaining_until(deadline)).with_context(|| {
        format!(
            "rootless Podman API service did not create a live socket at {}",
            paths.socket_path.display()
        )
    })
}

pub(in crate::guest_init) fn wait_for_socket(socket_path: &Path, timeout: Duration) -> Result<()> {
    let deadline = Instant::now() + timeout;
    wait_for_socket_until(socket_path, deadline)
}

fn wait_for_socket_until(socket_path: &Path, deadline: Instant) -> Result<()> {
    loop {
        if socket_is_live(socket_path) {
            return Ok(());
        }
        if Instant::now() >= deadline {
            bail!(
                "timed out waiting for Podman API socket {}",
                socket_path.display()
            );
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn remaining_until(deadline: Instant) -> Duration {
    deadline.saturating_duration_since(Instant::now())
}

pub(in crate::guest_init) fn socket_is_live(socket_path: &Path) -> bool {
    UnixStream::connect(socket_path).is_ok()
}

pub(in crate::guest_init) fn command_plan(
    identity: &DevIdentity,
    paths: &PodmanServicePaths,
) -> Result<PodmanServiceCommandPlan> {
    command_plan_with_env(identity, paths, &PodmanServiceEnv::from_process())
}

fn command_plan_with_env(
    identity: &DevIdentity,
    paths: &PodmanServicePaths,
    env: &PodmanServiceEnv,
) -> Result<PodmanServiceCommandPlan> {
    let program = resolve_real_podman_with_env(env)?;
    Ok(PodmanServiceCommandPlan {
        program,
        args: vec![
            "system".to_owned(),
            "service".to_owned(),
            "--time=0".to_owned(),
            paths.socket_uri.clone(),
        ],
        env: service_environment_with_env(identity, paths, env),
    })
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct PodmanServiceEnv {
    real_podman: Option<String>,
    search_path: Option<OsString>,
    service_path: String,
    ssl_cert_file: Option<String>,
    nix_ssl_cert_file: Option<String>,
}

impl PodmanServiceEnv {
    fn from_process() -> Self {
        Self {
            real_podman: std::env::var(REAL_PODMAN_ENV).ok(),
            search_path: std::env::var_os("PATH"),
            service_path: std::env::var("PATH").unwrap_or_default(),
            ssl_cert_file: std::env::var("SSL_CERT_FILE").ok(),
            nix_ssl_cert_file: std::env::var("NIX_SSL_CERT_FILE").ok(),
        }
    }
}

fn prepare_socket_dir(identity: &DevIdentity, paths: &PodmanServicePaths) -> Result<()> {
    if process::is_root() {
        crate::guest_init::components::rootless::runtime_dir::ensure_user_runtime_dir(identity)?;
    } else {
        guest_fs::create_dir_all(&paths.runtime_dir)?;
    }
    guest_fs::create_dir_all(&paths.socket_dir)?;
    if process::is_root() {
        guest_fs::chown(&paths.socket_dir, identity.uid, identity.gid)?;
    }
    guest_fs::chmod(&paths.socket_dir, 0o700)
}

fn resolve_real_podman_with_env(env: &PodmanServiceEnv) -> Result<PathBuf> {
    if let Some(path) = &env.real_podman {
        let path = PathBuf::from(path);
        validate_real_podman_candidate(&path)?;
        return Ok(path);
    }

    let path = env
        .search_path
        .as_ref()
        .ok_or_else(|| anyhow!("PATH is not set"))?;
    for dir in std::env::split_paths(path) {
        let candidate = dir.join("podman");
        if !command::is_executable(&candidate) {
            continue;
        }
        if validate_real_podman_candidate(&candidate).is_ok() {
            return Ok(candidate);
        }
    }
    bail!("could not find real podman binary; set {REAL_PODMAN_ENV} to the image Podman path")
}

fn validate_real_podman_candidate(path: &Path) -> Result<()> {
    if !command::is_executable(path) {
        bail!("Podman binary is not executable: {}", path.display());
    }
    if looks_like_loftd_wrapper(path)? {
        bail!(
            "refusing to start Podman API service through compatibility wrapper {}",
            path.display()
        );
    }
    Ok(())
}

fn looks_like_loftd_wrapper(path: &Path) -> Result<bool> {
    let mut file = File::open(path)
        .with_context(|| format!("failed to inspect Podman candidate {}", path.display()))?;
    let mut buf = vec![0; 8192];
    let read = file.read(&mut buf)?;
    buf.truncate(read);
    let text = String::from_utf8_lossy(&buf);
    Ok(text.contains("loftd-guest-init internal podman "))
}

fn service_environment_with_env(
    identity: &DevIdentity,
    paths: &PodmanServicePaths,
    process_env: &PodmanServiceEnv,
) -> Vec<(String, String)> {
    let home = identity.home.display().to_string();
    let mut env = vec![
        ("USER".to_owned(), DEV_USER.to_owned()),
        ("LOGNAME".to_owned(), DEV_USER.to_owned()),
        ("HOME".to_owned(), home.clone()),
        ("SHELL".to_owned(), identity.shell.display().to_string()),
        (
            "XDG_RUNTIME_DIR".to_owned(),
            paths.runtime_dir.display().to_string(),
        ),
        ("XDG_CONFIG_HOME".to_owned(), format!("{home}/.config")),
        ("XDG_DATA_HOME".to_owned(), format!("{home}/.local/share")),
        ("XDG_STATE_HOME".to_owned(), format!("{home}/.local/state")),
        ("XDG_CACHE_HOME".to_owned(), format!("{home}/.cache")),
        ("TMPDIR".to_owned(), format!("{home}/.cache/tmp")),
        (
            "PATH".to_owned(),
            format!("{WRAPPER_BIN_DIR}:{}", process_env.service_path),
        ),
    ];
    preserve_if_present(&mut env, "SSL_CERT_FILE", &process_env.ssl_cert_file);
    preserve_if_present(
        &mut env,
        "NIX_SSL_CERT_FILE",
        &process_env.nix_ssl_cert_file,
    );
    env
}

fn preserve_if_present(env: &mut Vec<(String, String)>, key: &str, value: &Option<String>) {
    if let Some(value) = value {
        env.push((key.to_owned(), value.clone()));
    }
}

fn spawn_service(identity: &DevIdentity, plan: &PodmanServiceCommandPlan) -> Result<()> {
    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(PODMAN_LOG_PATH)
        .with_context(|| format!("failed to open {}", PODMAN_LOG_PATH))?;
    if process::is_root() {
        guest_fs::chown(Path::new(PODMAN_LOG_PATH), identity.uid, identity.gid)?;
        guest_fs::chmod(Path::new(PODMAN_LOG_PATH), 0o644)?;
    }
    let log_err = log.try_clone()?;
    let mut command = Command::new(&plan.program);
    command
        .args(&plan.args)
        .env_clear()
        .envs(plan.env.iter().map(|(key, value)| (key, value)))
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(log_err));
    let uid = identity.uid;
    let gid = identity.gid;
    let starts_as_root = process::is_root();
    unsafe {
        command.pre_exec(move || {
            libc::setsid();
            if starts_as_root {
                if libc::setgroups(0, std::ptr::null()) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                if libc::setgid(gid) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                if libc::setuid(uid) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
            }
            Ok(())
        });
    }
    command
        .spawn()
        .with_context(|| format!("failed to spawn {}", plan.program.display()))?;
    Ok(())
}

fn remove_stale_socket(socket_path: &Path) -> Result<()> {
    match fs::remove_file(socket_path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err).with_context(|| {
            format!(
                "failed to remove stale Podman API socket {}",
                socket_path.display()
            )
        }),
    }
}

struct PodmanServiceLock {
    file: File,
}

impl PodmanServiceLock {
    fn acquire_until(
        identity: &DevIdentity,
        paths: &PodmanServicePaths,
        deadline: Instant,
    ) -> Result<Self> {
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&paths.lock_path)
            .with_context(|| format!("failed to open {}", paths.lock_path.display()))?;
        if process::is_root() {
            guest_fs::chown(&paths.lock_path, identity.uid, identity.gid)?;
        }
        loop {
            let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
            if rc == 0 {
                return Ok(Self { file });
            }
            let err = std::io::Error::last_os_error();
            if err.kind() != std::io::ErrorKind::WouldBlock
                && err.raw_os_error() != Some(libc::EWOULDBLOCK)
            {
                return Err(err)
                    .with_context(|| format!("failed to lock {}", paths.lock_path.display()));
            }
            if Instant::now() >= deadline {
                bail!(
                    "timed out waiting for Podman API service lock {}",
                    paths.lock_path.display()
                );
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

impl Drop for PodmanServiceLock {
    fn drop(&mut self) {
        let _ = unsafe { libc::flock(self.file.as_raw_fd(), libc::LOCK_UN) };
    }
}

#[cfg(test)]
#[path = "service_tests.rs"]
mod tests;
