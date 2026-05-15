use anyhow::{Context, Result, anyhow};
use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::guest_init) struct DockerStatus {
    pub(in crate::guest_init) state: DockerState,
    pub(in crate::guest_init) pid: Option<u32>,
    pub(in crate::guest_init) started_at: Option<u64>,
    pub(in crate::guest_init) finished_at: Option<u64>,
    pub(in crate::guest_init) log_path: Option<PathBuf>,
    pub(in crate::guest_init) error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::guest_init) enum DockerState {
    NotStarted,
    Running,
    Ready,
    Failed,
}

impl DockerStatus {
    pub(in crate::guest_init) fn not_started() -> Self {
        Self {
            state: DockerState::NotStarted,
            pid: None,
            started_at: None,
            finished_at: None,
            log_path: None,
            error: None,
        }
    }

    pub(in crate::guest_init) fn running(pid: u32, log_path: PathBuf) -> Self {
        Self {
            state: DockerState::Running,
            pid: Some(pid),
            started_at: Some(now_secs()),
            finished_at: None,
            log_path: Some(log_path),
            error: None,
        }
    }

    pub(in crate::guest_init) fn ready_from(running: &Self) -> Result<Self> {
        running.ensure_transition(DockerState::Ready)?;
        Ok(Self {
            state: DockerState::Ready,
            pid: running.pid,
            started_at: running.started_at,
            finished_at: Some(now_secs()),
            log_path: running.log_path.clone(),
            error: None,
        })
    }

    pub(in crate::guest_init) fn failed_from(
        running: &Self,
        error: impl Into<String>,
    ) -> Result<Self> {
        running.ensure_transition(DockerState::Failed)?;
        Ok(Self {
            state: DockerState::Failed,
            pid: running.pid,
            started_at: running.started_at,
            finished_at: Some(now_secs()),
            log_path: running.log_path.clone(),
            error: Some(error.into()),
        })
    }

    pub(in crate::guest_init) fn ensure_transition(&self, next: DockerState) -> Result<()> {
        let legal = matches!(
            (self.state, next),
            (DockerState::NotStarted, DockerState::Running)
                | (DockerState::Running, DockerState::Ready)
                | (DockerState::Running, DockerState::Failed)
        );
        if legal {
            Ok(())
        } else {
            Err(anyhow!(
                "illegal docker status transition from {} to {}",
                self.state.as_str(),
                next.as_str()
            ))
        }
    }

    pub(in crate::guest_init) fn to_text(&self) -> String {
        let mut out = format!("state={}\n", self.state.as_str());
        if let Some(pid) = self.pid {
            out.push_str(&format!("pid={pid}\n"));
        }
        if let Some(started_at) = self.started_at {
            out.push_str(&format!("started_at={started_at}\n"));
        }
        if let Some(finished_at) = self.finished_at {
            out.push_str(&format!("finished_at={finished_at}\n"));
        }
        if let Some(log_path) = &self.log_path {
            out.push_str(&format!("log_path={}\n", log_path.display()));
        }
        if let Some(error) = &self.error {
            out.push_str(&format!("error={}\n", escape_value(error)));
        }
        out
    }

    pub(in crate::guest_init) fn from_text(text: &str) -> Result<Self> {
        let mut status = Self::not_started();
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            match key {
                "state" => status.state = DockerState::from_str(value)?,
                "pid" => status.pid = Some(value.parse().context("invalid docker status pid")?),
                "started_at" => {
                    status.started_at =
                        Some(value.parse().context("invalid docker status started_at")?)
                }
                "finished_at" => {
                    status.finished_at =
                        Some(value.parse().context("invalid docker status finished_at")?)
                }
                "log_path" => status.log_path = Some(PathBuf::from(value)),
                "error" => status.error = Some(unescape_value(value)),
                _ => {}
            }
        }
        Ok(status)
    }
}

impl DockerState {
    pub(in crate::guest_init) fn as_str(self) -> &'static str {
        match self {
            Self::NotStarted => "not-started",
            Self::Running => "running",
            Self::Ready => "ready",
            Self::Failed => "failed",
        }
    }

    fn from_str(value: &str) -> Result<Self> {
        match value {
            "not-started" => Ok(Self::NotStarted),
            "running" => Ok(Self::Running),
            "ready" => Ok(Self::Ready),
            "failed" => Ok(Self::Failed),
            _ => Err(anyhow!("unknown docker state '{value}'")),
        }
    }
}

pub(in crate::guest_init) fn read_status(path: &Path) -> Result<DockerStatus> {
    match fs::read_to_string(path) {
        Ok(text) => DockerStatus::from_text(&text),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(DockerStatus::not_started()),
        Err(err) => Err(err).with_context(|| format!("failed to read {}", path.display())),
    }
}

#[cfg(test)]
pub(in crate::guest_init) fn write_status(path: &Path, status: &DockerStatus) -> Result<()> {
    with_status_lock(path, || write_status_unlocked(path, status))
}

pub(in crate::guest_init) fn write_running_unless_terminal(
    path: &Path,
    status: &DockerStatus,
) -> Result<bool> {
    with_status_lock(path, || {
        let current = read_status_unlocked(path)?;
        if matches!(current.state, DockerState::Ready | DockerState::Failed) {
            return Ok(false);
        }
        write_status_unlocked(path, status)?;
        Ok(true)
    })
}

pub(in crate::guest_init) fn write_running(path: &Path, status: &DockerStatus) -> Result<()> {
    with_status_lock(path, || write_status_unlocked(path, status))
}

pub(in crate::guest_init) fn mark_ready_for_pid(path: &Path, pid: u32) -> Result<()> {
    with_status_lock(path, || {
        let current = require_running_pid(path, pid)?;
        write_status_unlocked(path, &DockerStatus::ready_from(&current)?)
    })
}

pub(in crate::guest_init) fn mark_failed_for_pid(
    path: &Path,
    pid: u32,
    error: impl Into<String>,
) -> Result<()> {
    let error = error.into();
    with_status_lock(path, || {
        let current = require_running_pid(path, pid)?;
        write_status_unlocked(path, &DockerStatus::failed_from(&current, error.clone())?)
    })
}

fn require_running_pid(path: &Path, pid: u32) -> Result<DockerStatus> {
    let current = read_status_unlocked(path)?;
    if current.state == DockerState::Running && current.pid == Some(pid) {
        Ok(current)
    } else {
        Err(anyhow!(
            "docker status at {} is no longer running for pid {pid} (state={} pid={:?})",
            path.display(),
            current.state.as_str(),
            current.pid
        ))
    }
}

fn write_status_unlocked(path: &Path, status: &DockerStatus) -> Result<()> {
    crate::guest_init::fs::write_file(path, &status.to_text(), 0o644)
}

fn read_status_unlocked(path: &Path) -> Result<DockerStatus> {
    match fs::read_to_string(path) {
        Ok(text) => DockerStatus::from_text(&text),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(DockerStatus::not_started()),
        Err(err) => Err(err).with_context(|| format!("failed to read {}", path.display())),
    }
}

fn with_status_lock<T>(path: &Path, action: impl FnOnce() -> Result<T>) -> Result<T> {
    let lock_path = path.with_extension("lock");
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock_path)
        {
            Ok(lock) => {
                drop(lock);
                let result = action();
                let _ = fs::remove_file(&lock_path);
                return result;
            }
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                if Instant::now() >= deadline {
                    return Err(anyhow!(
                        "timed out waiting for docker status lock {}",
                        lock_path.display()
                    ));
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(err) => {
                return Err(err).with_context(|| {
                    format!(
                        "failed to acquire docker status lock {}",
                        lock_path.display()
                    )
                });
            }
        }
    }
}

pub(in crate::guest_init) fn format_wait_timeout(
    label: &str,
    path: &Path,
    status: &DockerStatus,
    timeout: Duration,
) -> String {
    let log = status
        .log_path
        .as_ref()
        .map(|path| format!("; log={}", path.display()))
        .unwrap_or_default();
    format!(
        "timed out after {}s waiting for rootless Docker {label} at {} (state={}{log})",
        timeout.as_secs(),
        path.display(),
        status.state.as_str()
    )
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn escape_value(value: &str) -> String {
    value.replace('\\', "\\\\").replace('\n', "\\n")
}

fn unescape_value(value: &str) -> String {
    let mut out = String::new();
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('\\') => out.push('\\'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(ch);
        }
    }
    out
}

#[cfg(test)]
#[path = "status_tests.rs"]
mod tests;
