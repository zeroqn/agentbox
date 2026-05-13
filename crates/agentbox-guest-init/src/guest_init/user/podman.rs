use anyhow::{bail, Result};
use std::path::Path;
use std::time::{Duration, Instant};

use crate::guest_init::process;
use crate::guest_init::runtime::libkrun::{PODMAN_STATUS_PATH, PODMAN_WAIT_TIMEOUT_SECS};
use crate::guest_init::status::{self, PodmanPrepState, PodmanPrepStatus};

pub(in crate::guest_init) fn wait_for_prep() -> Result<()> {
    wait_for_status(
        Path::new(PODMAN_STATUS_PATH),
        Duration::from_secs(PODMAN_WAIT_TIMEOUT_SECS),
        process::pid_alive,
    )
}

pub(in crate::guest_init) fn wait_for_status(
    status_path: &Path,
    timeout: Duration,
    pid_alive: impl Fn(u32) -> bool,
) -> Result<()> {
    let deadline = Instant::now() + timeout;
    loop {
        let status = status::read_status(status_path)?;
        if is_ready(&status) {
            return Ok(());
        }
        reject_failed_or_missing(status_path, &status)?;
        ensure_running_pid_is_live(status_path, &status, &pid_alive)?;
        ensure_wait_deadline(status_path, &status, timeout, deadline)?;
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn is_ready(status: &PodmanPrepStatus) -> bool {
    status.state == PodmanPrepState::Ready
}

fn reject_failed_or_missing(status_path: &Path, status: &PodmanPrepStatus) -> Result<()> {
    match status.state {
        PodmanPrepState::Failed => bail!(failed_message(status_path, status)),
        PodmanPrepState::NotStarted => bail!(
            "rootless Podman prep has not started; missing status at {}",
            status_path.display()
        ),
        PodmanPrepState::Ready | PodmanPrepState::Running => Ok(()),
    }
}

fn ensure_running_pid_is_live(
    status_path: &Path,
    status: &PodmanPrepStatus,
    pid_alive: &impl Fn(u32) -> bool,
) -> Result<()> {
    let Some(pid) = status.pid else {
        return Ok(());
    };
    if pid_alive(pid) {
        return Ok(());
    }
    bail!(
        "rootless Podman prep has stale/dead PID {pid}; status={}{}",
        status_path.display(),
        log_suffix(status)
    );
}

fn ensure_wait_deadline(
    status_path: &Path,
    status: &PodmanPrepStatus,
    timeout: Duration,
    deadline: Instant,
) -> Result<()> {
    if Instant::now() < deadline {
        return Ok(());
    }
    bail!(status::format_wait_timeout(status_path, status, timeout));
}

fn failed_message(status_path: &Path, status: &PodmanPrepStatus) -> String {
    let error = status.error.as_deref().unwrap_or("unknown error");
    format!(
        "rootless Podman prep failed: {error}; status={}{}",
        status_path.display(),
        log_suffix(status)
    )
}

fn log_suffix(status: &PodmanPrepStatus) -> String {
    status
        .log_path
        .as_ref()
        .map(|path| format!("; log={}", path.display()))
        .unwrap_or_default()
}

#[cfg(test)]
#[path = "podman_tests.rs"]
mod tests;
