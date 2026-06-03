use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::guest_init::components::env::{
    DEFAULT_SHELL, PODMAN_STATUS_PATH, PODMAN_WAIT_TIMEOUT_SECS,
};
use crate::guest_init::components::home::identity::DevIdentity;
use crate::guest_init::components::podman::status::{self, PodmanPrepState, PodmanPrepStatus};
use crate::guest_init::process;

pub(in crate::guest_init) fn wait_for_prep() -> Result<()> {
    let identity = current_identity();
    wait_for_prep_for_identity(&identity)
}

pub(in crate::guest_init) fn wait_for_prep_for_identity(_identity: &DevIdentity) -> Result<()> {
    wait_for_status(
        Path::new(PODMAN_STATUS_PATH),
        Duration::from_secs(PODMAN_WAIT_TIMEOUT_SECS),
        process::pid_alive,
    )
}

pub(in crate::guest_init) fn wait_for_service() -> Result<()> {
    let identity = current_identity();
    wait_for_service_for_identity(&identity)
}

pub(in crate::guest_init) fn wait_for_service_for_identity(identity: &DevIdentity) -> Result<()> {
    wait_for_status_with_service(
        Path::new(PODMAN_STATUS_PATH),
        Duration::from_secs(PODMAN_WAIT_TIMEOUT_SECS),
        process::pid_alive,
        || {
            crate::guest_init::components::podman::service::ensure_started(
                identity,
                Duration::from_secs(PODMAN_WAIT_TIMEOUT_SECS),
            )
        },
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
        if status.state == PodmanPrepState::Ready {
            return Ok(());
        }
        reject_failed_or_missing(status_path, &status)?;
        ensure_running_pid_is_live(status_path, &status, &pid_alive)?;
        ensure_wait_deadline(status_path, &status, timeout, deadline)?;
        std::thread::sleep(Duration::from_millis(100));
    }
}

pub(in crate::guest_init) fn wait_for_status_with_service(
    status_path: &Path,
    timeout: Duration,
    pid_alive: impl Fn(u32) -> bool,
    mut ensure_service: impl FnMut() -> Result<()>,
) -> Result<()> {
    wait_for_status(status_path, timeout, pid_alive)?;
    ensure_service().with_context(|| {
        format!(
            "rootless Podman prep is ready but API socket is not live; status={}",
            status_path.display()
        )
    })
}

fn current_identity() -> DevIdentity {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| DEFAULT_SHELL.to_owned());
    DevIdentity::new(process::uid(), process::gid(), PathBuf::from(shell))
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
#[path = "user_tests.rs"]
mod tests;
