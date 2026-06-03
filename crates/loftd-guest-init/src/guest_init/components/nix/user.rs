use anyhow::{Result, bail};
use std::os::unix::fs::FileTypeExt;
use std::path::Path;
use std::time::{Duration, Instant};

use crate::guest_init::components::env::{
    NIX_DAEMON_SOCKET_PATH, NIX_STATUS_PATH, NIX_WAIT_TIMEOUT_SECS,
};
use crate::guest_init::components::nix::status::{self, NixPrepState, NixPrepStatus};
use crate::guest_init::process;

pub(in crate::guest_init) fn wait_for_prep() -> Result<()> {
    wait_for_status_and_socket(
        Path::new(NIX_STATUS_PATH),
        Path::new(NIX_DAEMON_SOCKET_PATH),
        Duration::from_secs(NIX_WAIT_TIMEOUT_SECS),
        process::pid_alive,
    )
}

pub(in crate::guest_init) fn wait_for_status_and_socket(
    status_path: &Path,
    socket_path: &Path,
    timeout: Duration,
    pid_alive: impl Fn(u32) -> bool,
) -> Result<()> {
    let deadline = Instant::now() + timeout;
    let status = wait_for_ready_status(status_path, timeout, deadline, &pid_alive)?;
    wait_for_socket(status_path, socket_path, &status, timeout, deadline)
}

fn wait_for_ready_status(
    status_path: &Path,
    timeout: Duration,
    deadline: Instant,
    pid_alive: &impl Fn(u32) -> bool,
) -> Result<NixPrepStatus> {
    loop {
        let status = status::read_status(status_path)?;
        if status.state == NixPrepState::Ready {
            return Ok(status);
        }
        reject_failed_or_missing(status_path, &status)?;
        ensure_running_pid_is_live(status_path, &status, pid_alive)?;
        ensure_wait_deadline(status_path, &status, timeout, deadline)?;
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn wait_for_socket(
    status_path: &Path,
    socket_path: &Path,
    status: &NixPrepStatus,
    timeout: Duration,
    deadline: Instant,
) -> Result<()> {
    loop {
        match socket_state(socket_path) {
            SocketState::Socket => return Ok(()),
            state @ (SocketState::Missing | SocketState::Other(_)) => {
                if Instant::now() >= deadline {
                    bail!(socket_timeout_message(
                        status_path,
                        socket_path,
                        status,
                        timeout,
                        state,
                    ));
                }
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn reject_failed_or_missing(status_path: &Path, status: &NixPrepStatus) -> Result<()> {
    match status.state {
        NixPrepState::Failed => bail!(failed_message(status_path, status)),
        NixPrepState::NotStarted => bail!(
            "internal /nix overlay prep has not started; missing status at {}",
            status_path.display()
        ),
        NixPrepState::Ready | NixPrepState::Running => Ok(()),
    }
}

fn ensure_running_pid_is_live(
    status_path: &Path,
    status: &NixPrepStatus,
    pid_alive: &impl Fn(u32) -> bool,
) -> Result<()> {
    let Some(pid) = status.pid else {
        return Ok(());
    };
    if pid_alive(pid) {
        return Ok(());
    }
    bail!(
        "internal /nix overlay prep has stale/dead PID {pid}; status={}{}",
        status_path.display(),
        log_suffix(status)
    );
}

fn ensure_wait_deadline(
    status_path: &Path,
    status: &NixPrepStatus,
    timeout: Duration,
    deadline: Instant,
) -> Result<()> {
    if Instant::now() < deadline {
        return Ok(());
    }
    bail!(status::format_wait_timeout(status_path, status, timeout));
}

fn failed_message(status_path: &Path, status: &NixPrepStatus) -> String {
    let error = status.error.as_deref().unwrap_or("unknown error");
    format!(
        "internal /nix overlay prep failed: {error}; status={}{}",
        status_path.display(),
        log_suffix(status)
    )
}

fn socket_timeout_message(
    status_path: &Path,
    socket_path: &Path,
    status: &NixPrepStatus,
    timeout: Duration,
    socket_state: SocketState,
) -> String {
    format!(
        "timed out after {}s waiting for internal nix-daemon Unix socket at {} \
         (observed={}); status={}{}",
        timeout.as_secs(),
        socket_path.display(),
        socket_state.as_str(),
        status_path.display(),
        log_suffix(status)
    )
}

fn log_suffix(status: &NixPrepStatus) -> String {
    status
        .log_path
        .as_ref()
        .map(|path| format!("; log={}", path.display()))
        .unwrap_or_default()
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SocketState {
    Missing,
    Socket,
    Other(&'static str),
}

impl SocketState {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Missing => "missing",
            Self::Socket => "socket",
            Self::Other(kind) => kind,
        }
    }
}

fn socket_state(path: &Path) -> SocketState {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) => {
            let file_type = metadata.file_type();
            if file_type.is_socket() {
                SocketState::Socket
            } else if file_type.is_file() {
                SocketState::Other("regular-file")
            } else if file_type.is_dir() {
                SocketState::Other("directory")
            } else if file_type.is_symlink() {
                SocketState::Other("symlink")
            } else {
                SocketState::Other("non-socket")
            }
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => SocketState::Missing,
        Err(_) => SocketState::Other("unreadable"),
    }
}

#[cfg(test)]
#[path = "user_tests.rs"]
mod tests;
