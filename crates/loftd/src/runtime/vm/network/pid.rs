//! Pipe and pid lifecycle helpers for VM/network workers.

use anyhow::{Context, Result, bail};
use std::fs;
use std::io::{Read, Write};
use std::os::fd::{FromRawFd, OwnedFd};
use std::thread;
use std::time::{Duration, Instant};

const STARTUP_POLL: Duration = Duration::from_millis(25);

pub(crate) fn passt_pid_pipe() -> Result<(OwnedFd, OwnedFd)> {
    pipe_cloexec("loftd passt pid pipe")
}

pub(super) fn pipe_cloexec(label: &str) -> Result<(OwnedFd, OwnedFd)> {
    let mut fds = [0; 2];
    // SAFETY: pipe2 writes two valid close-on-exec fds on success.
    let rc = unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) };
    if rc < 0 {
        bail!(
            "failed to create {label}: {}",
            std::io::Error::last_os_error()
        );
    }
    // SAFETY: fds are freshly returned by pipe and uniquely owned here.
    let read_fd = unsafe { OwnedFd::from_raw_fd(fds[0]) };
    // SAFETY: fds are freshly returned by pipe and uniquely owned here.
    let write_fd = unsafe { OwnedFd::from_raw_fd(fds[1]) };
    Ok((read_fd, write_fd))
}

pub(crate) fn write_passt_pid(fd: OwnedFd, pid: libc::pid_t) -> Result<()> {
    let mut file = fs::File::from(fd);
    file.write_all(pid.to_string().as_bytes())
        .context("failed to send loftd passt pid to network manager")
}

pub(crate) fn read_passt_pid(fd: OwnedFd) -> Result<Option<libc::pid_t>> {
    let mut file = fs::File::from(fd);
    let mut text = String::new();
    file.read_to_string(&mut text)
        .context("failed to read loftd passt pid from VM worker")?;
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    Ok(Some(
        text.parse::<libc::pid_t>()
            .context("loftd VM worker reported invalid passt pid")?,
    ))
}

pub(crate) fn wait_pid(pid: libc::pid_t) -> Result<i32> {
    let mut status = 0;
    // SAFETY: pid is a child process id returned by fork.
    let rc = unsafe { libc::waitpid(pid, &mut status, 0) };
    if rc < 0 {
        bail!(
            "failed to wait for loftd VM worker {pid}: {}",
            std::io::Error::last_os_error()
        );
    }
    Ok(status)
}

pub(crate) fn status_exit_code(status: i32) -> Option<i32> {
    if libc::WIFEXITED(status) {
        Some(libc::WEXITSTATUS(status))
    } else {
        None
    }
}

pub(crate) fn kill_and_wait_pid(pid: libc::pid_t) {
    // SAFETY: best-effort signal to a child/descendant pid managed by loftd.
    let _ = unsafe { libc::kill(pid, libc::SIGTERM) };
    let start = Instant::now();
    loop {
        let mut status = 0;
        // SAFETY: best-effort nonblocking reap of a known pid.
        let rc = unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) };
        if rc == pid || rc < 0 {
            break;
        }
        if start.elapsed() > Duration::from_millis(500) {
            // SAFETY: escalate cleanup for stubborn child.
            let _ = unsafe { libc::kill(pid, libc::SIGKILL) };
            // SAFETY: final blocking reap.
            let _ = unsafe { libc::waitpid(pid, &mut status, 0) };
            break;
        }
        thread::sleep(STARTUP_POLL);
    }
}

pub(crate) fn cleanup_pid(pid: libc::pid_t) {
    kill_and_wait_pid(pid);
}
