//! One-shot helper-to-parent readiness pipe for managed attach startup.

use anyhow::{Context, Result, anyhow, bail};
use std::env;
use std::ffi::OsString;
use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::process::Child;
use std::time::{Duration, Instant};

pub(crate) const READY_FD_ENV: &str = "LOFTD_MANAGED_READY_FD";

const READY_FRAME: &[u8] = b"READY\n";
const MAX_HEADER_LEN: usize = 64;
const MAX_ERROR_PAYLOAD_LEN: usize = 16 * 1024;
const WAIT_POLL: Duration = Duration::from_millis(25);

pub(crate) struct ParentReadyPipe {
    reader: File,
    writer: Option<OwnedFd>,
}

impl ParentReadyPipe {
    pub(crate) fn create() -> Result<Self> {
        let mut fds = [-1; 2];
        // SAFETY: pipe writes two valid file descriptors into `fds` on success.
        let rc = unsafe { libc::pipe(fds.as_mut_ptr()) };
        if rc < 0 {
            bail!(
                "failed to create managed attach readiness pipe: {}",
                std::io::Error::last_os_error()
            );
        }
        // SAFETY: both fds are freshly returned and uniquely owned here.
        let read_fd = unsafe { OwnedFd::from_raw_fd(fds[0]) };
        // SAFETY: both fds are freshly returned and uniquely owned here.
        let write_fd = unsafe { OwnedFd::from_raw_fd(fds[1]) };
        set_close_on_exec(read_fd.as_raw_fd())
            .context("failed to mark managed attach readiness reader close-on-exec")?;
        // SAFETY: File takes unique ownership of the read fd.
        let reader = File::from(read_fd);
        Ok(Self {
            reader,
            writer: Some(write_fd),
        })
    }

    pub(crate) fn writer_fd(&self) -> Option<RawFd> {
        self.writer.as_ref().map(AsRawFd::as_raw_fd)
    }

    pub(crate) fn close_parent_writer(&mut self) {
        self.writer.take();
    }

    pub(crate) fn wait_for_ready(&mut self, child: &mut Child, timeout: Duration) -> Result<()> {
        self.close_parent_writer();
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(status) = child
                .try_wait()
                .context("failed to poll loftd helper while waiting for managed attach readiness")?
            {
                bail!("loftd helper exited with {status} before managed attach socket was ready");
            }
            match self.read_status_before(deadline)? {
                ReadStatus::Ready => return Ok(()),
                ReadStatus::Error(message) => {
                    bail!("managed attach socket readiness failed: {message}")
                }
                ReadStatus::Eof => bail!("managed attach readiness pipe closed before READY"),
                ReadStatus::Pending => {
                    if Instant::now() >= deadline {
                        bail!("timed out waiting for managed attach socket readiness");
                    }
                }
            }
        }
    }

    fn read_status_before(&mut self, deadline: Instant) -> Result<ReadStatus> {
        if !poll_readable_once(self.reader.as_raw_fd(), deadline, WAIT_POLL)? {
            return Ok(ReadStatus::Pending);
        }
        read_status_frame(&mut self.reader, deadline)
    }
}

pub(crate) struct HelperReadyWriter {
    file: File,
}

impl HelperReadyWriter {
    pub(crate) fn from_env() -> Result<Option<Self>> {
        let Some(value) = env::var_os(READY_FD_ENV) else {
            return Ok(None);
        };
        let fd = parse_fd_env(value)?;
        if fd < 0 {
            bail!("{READY_FD_ENV} must be a non-negative file descriptor");
        }
        // SAFETY: the helper owns the inherited fd named by READY_FD_ENV. This
        // wrapper closes it on drop after sending a terminal readiness frame.
        let file = unsafe { File::from_raw_fd(fd) };
        set_close_on_exec(file.as_raw_fd())
            .context("failed to mark managed attach readiness fd close-on-exec in helper")?;
        Ok(Some(Self { file }))
    }

    pub(crate) fn close_in_vm_worker_child_from_env() {
        let Some(value) = env::var_os(READY_FD_ENV) else {
            return;
        };
        let Ok(fd) = parse_fd_env(value) else {
            return;
        };
        if fd >= 0 {
            // SAFETY: best-effort close in the forked VM worker child so it
            // cannot keep the helper readiness pipe alive.
            let _ = unsafe { libc::close(fd) };
        }
    }

    pub(crate) fn send_ready(mut self) -> Result<()> {
        self.file
            .write_all(READY_FRAME)
            .context("failed to write managed attach READY frame")
    }

    pub(crate) fn send_error(mut self, message: &str) -> Result<()> {
        let payload = bounded_payload(message);
        let header = format!("ERR {}\n", payload.len());
        self.file
            .write_all(header.as_bytes())
            .context("failed to write managed attach ERR frame header")?;
        self.file
            .write_all(payload.as_bytes())
            .context("failed to write managed attach ERR frame payload")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ReadStatus {
    Ready,
    Error(String),
    Eof,
    Pending,
}

fn read_status_frame(reader: &mut File, deadline: Instant) -> Result<ReadStatus> {
    let mut header = Vec::new();
    loop {
        let Some(byte) = read_byte_before(reader, deadline)? else {
            if header.is_empty() {
                return Ok(ReadStatus::Eof);
            }
            bail!("managed attach readiness pipe closed mid-frame");
        };
        if byte == b'\n' {
            break;
        }
        header.push(byte);
        if header.len() > MAX_HEADER_LEN {
            bail!("managed attach readiness frame header is too long");
        }
    }
    if header == b"READY" {
        return Ok(ReadStatus::Ready);
    }
    let header = std::str::from_utf8(&header)
        .context("managed attach readiness frame header is not UTF-8")?;
    let Some(len_text) = header.strip_prefix("ERR ") else {
        bail!("unknown managed attach readiness frame '{header}'");
    };
    let len = len_text
        .parse::<usize>()
        .with_context(|| format!("invalid managed attach ERR payload length '{len_text}'"))?;
    if len > MAX_ERROR_PAYLOAD_LEN {
        bail!("managed attach ERR payload length {len} exceeds limit {MAX_ERROR_PAYLOAD_LEN}");
    }
    let mut payload = vec![0u8; len];
    read_exact_before(reader, &mut payload, deadline)?;
    let message =
        String::from_utf8(payload).context("managed attach ERR payload is not valid UTF-8")?;
    Ok(ReadStatus::Error(message))
}

fn read_exact_before(reader: &mut File, buf: &mut [u8], deadline: Instant) -> Result<()> {
    for slot in buf {
        let Some(byte) = read_byte_before(reader, deadline)? else {
            bail!("managed attach readiness pipe closed mid-payload");
        };
        *slot = byte;
    }
    Ok(())
}

fn read_byte_before(reader: &mut File, deadline: Instant) -> Result<Option<u8>> {
    if !poll_readable_until(reader.as_raw_fd(), deadline, WAIT_POLL)? {
        bail!("timed out reading managed attach readiness frame");
    }
    let mut byte = [0u8; 1];
    match reader.read(&mut byte) {
        Ok(0) => Ok(None),
        Ok(1) => Ok(Some(byte[0])),
        Ok(_) => unreachable!("single-byte read returned more than one byte"),
        Err(err) if err.kind() == std::io::ErrorKind::Interrupted => {
            read_byte_before(reader, deadline)
        }
        Err(err) => Err(err).context("failed to read managed attach readiness pipe"),
    }
}

fn poll_readable_once(fd: RawFd, deadline: Instant, poll_slice: Duration) -> Result<bool> {
    let now = Instant::now();
    if now >= deadline {
        return Ok(false);
    }
    let timeout = poll_slice.min(deadline.saturating_duration_since(now));
    poll_readable_for(fd, timeout)
}

fn poll_readable_until(fd: RawFd, deadline: Instant, poll_slice: Duration) -> Result<bool> {
    loop {
        let now = Instant::now();
        if now >= deadline {
            return Ok(false);
        }
        let timeout = poll_slice.min(deadline.saturating_duration_since(now));
        if poll_readable_for(fd, timeout)? {
            return Ok(true);
        }
    }
}

fn poll_readable_for(fd: RawFd, timeout: Duration) -> Result<bool> {
    let mut pollfd = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };
    loop {
        // SAFETY: poll is called with one valid pollfd.
        let rc = unsafe { libc::poll(&mut pollfd, 1, duration_ms_i32(timeout)) };
        if rc > 0 {
            return Ok(true);
        }
        if rc == 0 {
            return Ok(false);
        }
        let err = std::io::Error::last_os_error();
        if err.kind() != std::io::ErrorKind::Interrupted {
            return Err(err).context("failed to poll managed attach readiness pipe");
        }
    }
}

fn duration_ms_i32(duration: Duration) -> i32 {
    i32::try_from(duration.as_millis())
        .unwrap_or(i32::MAX)
        .max(1)
}

fn parse_fd_env(value: OsString) -> Result<RawFd> {
    let text = value
        .into_string()
        .map_err(|_| anyhow!("{READY_FD_ENV} is not valid UTF-8"))?;
    text.parse::<RawFd>()
        .with_context(|| format!("{READY_FD_ENV} value '{text}' is not a file descriptor"))
}

fn set_close_on_exec(fd: RawFd) -> Result<()> {
    // SAFETY: fcntl is called for a valid inherited readiness fd.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
    if flags < 0 {
        return Err(std::io::Error::last_os_error()).context("failed to read readiness fd flags");
    }
    // SAFETY: fcntl updates only the close-on-exec flag on the readiness fd.
    let rc = unsafe { libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC) };
    if rc == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error()).context("failed to set readiness fd close-on-exec")
    }
}

fn bounded_payload(message: &str) -> String {
    if message.len() <= MAX_ERROR_PAYLOAD_LEN {
        return message.to_owned();
    }
    let mut end = MAX_ERROR_PAYLOAD_LEN;
    while !message.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &message[..end.saturating_sub('…'.len_utf8())])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn reads_ready_frame() {
        let mut pipe = ParentReadyPipe::create().unwrap();
        let mut writer = File::from(pipe.writer.take().unwrap());
        writer.write_all(b"READY\n").unwrap();
        drop(writer);

        assert_eq!(
            pipe.read_status_before(Instant::now() + Duration::from_secs(1))
                .unwrap(),
            ReadStatus::Ready
        );
    }

    #[test]
    fn reads_length_tagged_error_frame() {
        let mut pipe = ParentReadyPipe::create().unwrap();
        let mut writer = File::from(pipe.writer.take().unwrap());
        writer.write_all(b"ERR 11\nhello world").unwrap();
        drop(writer);

        assert_eq!(
            pipe.read_status_before(Instant::now() + Duration::from_secs(1))
                .unwrap(),
            ReadStatus::Error("hello world".to_owned())
        );
    }

    #[test]
    fn reports_eof_before_frame() {
        let mut pipe = ParentReadyPipe::create().unwrap();
        drop(pipe.writer.take());

        assert_eq!(
            pipe.read_status_before(Instant::now() + Duration::from_secs(1))
                .unwrap(),
            ReadStatus::Eof
        );
    }

    #[test]
    fn rejects_malformed_frame() {
        let mut pipe = ParentReadyPipe::create().unwrap();
        let mut writer = File::from(pipe.writer.take().unwrap());
        writer.write_all(b"WAT\n").unwrap();
        drop(writer);

        let err = pipe
            .read_status_before(Instant::now() + Duration::from_secs(1))
            .unwrap_err();
        assert!(format!("{err:#}").contains("unknown managed attach readiness frame"));
    }
}
