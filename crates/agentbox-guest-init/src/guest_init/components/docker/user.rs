use anyhow::{Context, Result, bail};
use std::fs::OpenOptions;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::guest_init::command;
use crate::guest_init::components::docker::config::DockerPaths;
use crate::guest_init::components::docker::status::{self, DockerState, DockerStatus};
use crate::guest_init::components::env::{
    DEV_HOME, DOCKER_DAEMON_TIMEOUT_SECS, DOCKER_PREP_STATUS_PATH, DOCKER_WAIT_TIMEOUT_SECS,
};
use crate::guest_init::components::home::identity::DevIdentity;
use crate::guest_init::process;

pub(in crate::guest_init) fn wait_for_prep() -> Result<()> {
    wait_for_status(
        "prep",
        Path::new(DOCKER_PREP_STATUS_PATH),
        Duration::from_secs(DOCKER_WAIT_TIMEOUT_SECS),
        process::pid_alive,
    )
}

pub(in crate::guest_init) fn ensure_daemon() -> Result<()> {
    wait_for_prep()?;
    if process::is_root() {
        bail!("rootless Docker daemon must be started as the dev user, not root");
    }
    let uid = process::uid();
    let gid = process::gid();
    let identity = DevIdentity::new(uid, gid, PathBuf::from("fish"));
    let paths = DockerPaths::for_identity(&identity);
    ensure_runtime_dir(&paths, uid, gid)?;
    match decide_daemon_start(&paths)? {
        DaemonStartDecision::Ready => Ok(()),
        DaemonStartDecision::WaitForExisting => wait_for_existing_daemon(&paths),
        DaemonStartDecision::Started(pid) => {
            wait_for_daemon_ready(&paths, paths.daemon_status_path.as_path(), pid)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::guest_init) enum DaemonStartDecision {
    Ready,
    WaitForExisting,
    Started(u32),
}

fn decide_daemon_start(paths: &DockerPaths) -> Result<DaemonStartDecision> {
    with_daemon_start_lock(paths, || {
        if validate_docker_info(paths).is_ok() {
            return Ok(DaemonStartDecision::Ready);
        }
        if has_live_starting_daemon(paths.daemon_status_path.as_path())? {
            return Ok(DaemonStartDecision::WaitForExisting);
        }
        cleanup_stale_daemon_files(paths)?;
        start_daemon(paths).map(DaemonStartDecision::Started)
    })
}

fn wait_for_existing_daemon(paths: &DockerPaths) -> Result<()> {
    wait_for_status(
        "daemon",
        paths.daemon_status_path.as_path(),
        Duration::from_secs(DOCKER_DAEMON_TIMEOUT_SECS),
        process::pid_alive,
    )?;
    validate_docker_info(paths)
}

pub(in crate::guest_init) fn has_live_starting_daemon(status_path: &Path) -> Result<bool> {
    let current = status::read_status(status_path)?;
    Ok(current.state == DockerState::Running && current.pid.is_some_and(process::pid_alive))
}

pub(in crate::guest_init) fn wait_for_status(
    label: &str,
    status_path: &Path,
    timeout: Duration,
    pid_alive: impl Fn(u32) -> bool,
) -> Result<()> {
    let deadline = Instant::now() + timeout;
    loop {
        let status = status::read_status(status_path)?;
        if status.state == DockerState::Ready {
            return Ok(());
        }
        reject_failed_or_missing(label, status_path, &status)?;
        ensure_running_pid_is_live(label, status_path, &status, &pid_alive)?;
        ensure_wait_deadline(label, status_path, &status, timeout, deadline)?;
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn start_daemon(paths: &DockerPaths) -> Result<u32> {
    command::require_on_path("dockerd-rootless.sh")?;
    let status_path = paths.daemon_status_path.clone();
    let log_path = paths.daemon_log_path.clone();
    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .with_context(|| format!("failed to open {}", log_path.display()))?;
    let log_err = log.try_clone()?;
    let child = unsafe {
        Command::new("dockerd-rootless.sh")
            .args([
                "--config-file",
                path_str(&paths.daemon_config)?,
                "--host",
                &paths.host_uri(),
            ])
            .envs(daemon_env(paths))
            .stdout(Stdio::from(log))
            .stderr(Stdio::from(log_err))
            .pre_exec(|| {
                libc::setsid();
                Ok(())
            })
            .spawn()
    }
    .context("failed to spawn rootless Docker daemon")?;
    let pid = child.id();
    status::write_running(&status_path, &DockerStatus::running(pid, log_path))?;
    Ok(pid)
}

fn wait_for_daemon_ready(paths: &DockerPaths, status_path: &Path, pid: u32) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(DOCKER_DAEMON_TIMEOUT_SECS);
    loop {
        match validate_docker_info(paths) {
            Ok(()) => return status::mark_ready_for_pid(status_path, pid),
            Err(err) => {
                if !process::pid_alive(pid) {
                    let message =
                        format!("rootless Docker daemon exited before readiness: {err:#}");
                    let _ = status::mark_failed_for_pid(status_path, pid, message.clone());
                    bail!(message);
                }
                if Instant::now() >= deadline {
                    let running = status::read_status(status_path)?;
                    let message = status::format_wait_timeout(
                        "daemon",
                        status_path,
                        &running,
                        Duration::from_secs(DOCKER_DAEMON_TIMEOUT_SECS),
                    );
                    let _ = status::mark_failed_for_pid(status_path, pid, message.clone());
                    bail!(message);
                }
            }
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

fn validate_docker_info(paths: &DockerPaths) -> Result<()> {
    let output = Command::new("docker")
        .args([
            "--host",
            &paths.host_uri(),
            "info",
            "--format",
            "{{.Driver}}|{{.DockerRootDir}}|{{.CgroupDriver}}",
        ])
        .envs(daemon_env(paths))
        .env_remove("AGENTBOX_LIBKRUN_CONTAINERS_STORAGE")
        .stderr(Stdio::null())
        .output()
        .context("failed to run docker info")?;
    if !output.status.success() {
        bail!("docker info exited with status {}", output.status);
    }
    validate_info_line(&String::from_utf8_lossy(&output.stdout), paths)
}

pub(in crate::guest_init) fn validate_info_line(output: &str, paths: &DockerPaths) -> Result<()> {
    let fields = output.trim();
    let parts = fields.split('|').collect::<Vec<_>>();
    if parts.len() != 3 {
        bail!("docker info returned unexpected storage summary '{fields}'");
    }
    if parts[0] != "btrfs" {
        bail!(
            "rootless Docker must use btrfs storage driver, got '{}'",
            parts[0]
        );
    }
    if parts[1] != paths.data_root.to_string_lossy() {
        bail!(
            "rootless Docker data-root escaped containers disk: got '{}', expected '{}'",
            parts[1],
            paths.data_root.display()
        );
    }
    if !parts[2].is_empty() && parts[2] != "none" {
        bail!(
            "rootless Docker cgroup driver must be none in libkrun, got '{}'",
            parts[2]
        );
    }
    Ok(())
}

fn daemon_env(paths: &DockerPaths) -> Vec<(String, String)> {
    let path = std::env::var("PATH").unwrap_or_default();
    vec![
        ("HOME".to_owned(), DEV_HOME.to_owned()),
        ("XDG_CONFIG_HOME".to_owned(), format!("{DEV_HOME}/.config")),
        (
            "XDG_DATA_HOME".to_owned(),
            format!("{DEV_HOME}/.local/share"),
        ),
        (
            "XDG_STATE_HOME".to_owned(),
            format!("{DEV_HOME}/.local/state"),
        ),
        ("XDG_CACHE_HOME".to_owned(), format!("{DEV_HOME}/.cache")),
        (
            "XDG_RUNTIME_DIR".to_owned(),
            paths
                .runtime_dir
                .parent()
                .unwrap_or_else(|| Path::new("/run/user/0"))
                .display()
                .to_string(),
        ),
        ("DOCKER_HOST".to_owned(), paths.host_uri()),
        ("PATH".to_owned(), format!("/run/agentbox/idmap-bin:{path}")),
    ]
}

fn cleanup_stale_daemon_files(paths: &DockerPaths) -> Result<()> {
    for path in [&paths.socket_path, &paths.pid_path] {
        match std::fs::remove_file(path) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => {
                return Err(err)
                    .with_context(|| format!("failed to remove stale {}", path.display()));
            }
        }
    }
    Ok(())
}

fn with_daemon_start_lock<T>(paths: &DockerPaths, action: impl FnOnce() -> Result<T>) -> Result<T> {
    let lock_path = paths.daemon_start_lock_path.as_path();
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(lock_path)
        {
            Ok(lock) => {
                drop(lock);
                let result = action();
                let _ = std::fs::remove_file(lock_path);
                return result;
            }
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                if Instant::now() >= deadline {
                    bail!(
                        "timed out waiting for rootless Docker daemon start lock {}",
                        lock_path.display()
                    );
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(err) => {
                return Err(err).with_context(|| {
                    format!(
                        "failed to acquire rootless Docker daemon start lock {}",
                        lock_path.display()
                    )
                });
            }
        }
    }
}

fn ensure_runtime_dir(paths: &DockerPaths, _uid: u32, _gid: u32) -> Result<()> {
    crate::guest_init::fs::create_dir_all(&paths.runtime_dir)?;
    crate::guest_init::fs::chmod(&paths.runtime_dir, 0o700)
}

fn reject_failed_or_missing(label: &str, status_path: &Path, current: &DockerStatus) -> Result<()> {
    match current.state {
        DockerState::Failed => bail!(failed_message(label, status_path, current)),
        DockerState::NotStarted => bail!(
            "rootless Docker {label} has not started; missing status at {}",
            status_path.display()
        ),
        DockerState::Ready | DockerState::Running => Ok(()),
    }
}

fn ensure_running_pid_is_live(
    label: &str,
    status_path: &Path,
    current: &DockerStatus,
    pid_alive: &impl Fn(u32) -> bool,
) -> Result<()> {
    let Some(pid) = current.pid else {
        return Ok(());
    };
    if pid_alive(pid) {
        return Ok(());
    }
    bail!(
        "rootless Docker {label} has stale/dead PID {pid}; status={}{}",
        status_path.display(),
        log_suffix(current)
    );
}

fn ensure_wait_deadline(
    label: &str,
    status_path: &Path,
    current: &DockerStatus,
    timeout: Duration,
    deadline: Instant,
) -> Result<()> {
    if Instant::now() < deadline {
        return Ok(());
    }
    bail!(status::format_wait_timeout(
        label,
        status_path,
        current,
        timeout
    ));
}

fn failed_message(label: &str, status_path: &Path, current: &DockerStatus) -> String {
    let error = current.error.as_deref().unwrap_or("unknown error");
    format!(
        "rootless Docker {label} failed: {error}; status={}{}",
        status_path.display(),
        log_suffix(current)
    )
}

fn log_suffix(current: &DockerStatus) -> String {
    current
        .log_path
        .as_ref()
        .map(|path| format!("; log={}", path.display()))
        .unwrap_or_default()
}

fn path_str(path: &Path) -> Result<&str> {
    path.to_str()
        .ok_or_else(|| anyhow::anyhow!("path is not valid UTF-8: {}", path.display()))
}

#[cfg(test)]
#[path = "user_tests.rs"]
mod tests;
