use anyhow::{Context, Result, anyhow, bail};
use loftd_attach_protocol::{DetachFilter, Frame, PROTOCOL_VERSION, read_frame, write_frame};
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::runtime::session::task_control::{
    ActiveTaskStatus, ProcessInspector, list_records, resolve_task_selector,
};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const CONNECT_RETRY: Duration = Duration::from_millis(50);
const IO_BUF_SIZE: usize = 16 * 1024;
const ATTACH_IO_POLL_MS: i32 = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AttachOutcome {
    Detached,
    Exited(i32),
}

impl AttachOutcome {
    pub(crate) fn message(self) -> String {
        match self {
            Self::Detached => "loftd: detached\n".to_owned(),
            Self::Exited(code) => format!("loftd: session exited with status {code}\n"),
        }
    }
}

pub(crate) fn attach_to_task(
    app_dir: &Path,
    task_selector: &str,
    inspector: &impl ProcessInspector,
) -> Result<String> {
    let records = list_records(app_dir)?;
    let record = resolve_task_selector(&records, task_selector)?;
    match inspector.status(&record.process) {
        ActiveTaskStatus::Running => {}
        status => bail!(
            "loftd task '{}' is not attachable because it is {}",
            record.task_id,
            status.as_str()
        ),
    }
    let managed = record.managed.as_ref().ok_or_else(|| {
        anyhow!(
            "loftd task '{}' is not a managed attachable session",
            record.task_id
        )
    })?;
    let outcome = attach_to_socket(&managed.attach_socket)?;
    Ok(outcome.message())
}

pub(crate) fn attach_to_socket(socket_path: &Path) -> Result<AttachOutcome> {
    let stream = connect_with_retry(socket_path)?;
    attach_stream(stream)
}

fn connect_with_retry(socket_path: &Path) -> Result<UnixStream> {
    let deadline = Instant::now() + CONNECT_TIMEOUT;
    let mut last_error = None;
    while Instant::now() <= deadline {
        match UnixStream::connect(socket_path) {
            Ok(stream) => return Ok(stream),
            Err(err) => {
                last_error = Some(err);
                thread::sleep(CONNECT_RETRY);
            }
        }
    }
    Err(last_error.unwrap_or_else(|| std::io::Error::new(std::io::ErrorKind::TimedOut, "timeout")))
        .with_context(|| {
            format!(
                "failed to connect to loftd attach socket '{}'",
                socket_path.display()
            )
        })
}

fn attach_stream(mut stream: UnixStream) -> Result<AttachOutcome> {
    match read_frame(&mut stream)? {
        Some(Frame::Hello { version }) if version == PROTOCOL_VERSION => {}
        Some(Frame::Hello { version }) => bail!(
            "loftd attach protocol version mismatch: host supports {PROTOCOL_VERSION}, guest reported {version}"
        ),
        Some(Frame::Busy) => bail!("loftd task already has an attached client"),
        Some(Frame::Error(message)) => bail!("loftd attach failed: {message}"),
        Some(frame) => bail!("unexpected first loftd attach frame: {frame:?}"),
        None => bail!("loftd attach socket closed before handshake"),
    }
    write_frame(&mut stream, &Frame::Attach)?;
    if let Some(size) = terminal_size(libc::STDIN_FILENO) {
        write_frame(
            &mut stream,
            &Frame::Resize {
                rows: size.rows,
                cols: size.cols,
            },
        )?;
    }

    let _raw = RawTerminalMode::enter(libc::STDIN_FILENO)?;
    let writer = Arc::new(Mutex::new(stream.try_clone()?));
    let active = Arc::new(AtomicBool::new(true));
    let stdin_writer = writer.clone();
    let stdin_active = active.clone();
    let stdin_thread = thread::spawn(move || proxy_stdin(stdin_writer, stdin_active));

    let outcome = proxy_remote(stream);
    active.store(false, Ordering::Release);
    let _ = stdin_thread.join();
    outcome
}

fn proxy_stdin(writer: Arc<Mutex<UnixStream>>, active: Arc<AtomicBool>) -> Result<()> {
    let mut filter = DetachFilter::default();
    let mut input = std::io::stdin().lock();
    let mut buf = [0u8; IO_BUF_SIZE];
    let mut last_size = terminal_size(libc::STDIN_FILENO);
    while active.load(Ordering::Acquire) {
        if let Some(size) = terminal_size_changed(&mut last_size) {
            write_to_guest(
                &writer,
                &Frame::Resize {
                    rows: size.rows,
                    cols: size.cols,
                },
            )?;
        }
        match wait_for_stdin(ATTACH_IO_POLL_MS)? {
            StdinReadiness::TimedOut => continue,
            StdinReadiness::Readable => {}
        }
        let n = match input.read(&mut buf) {
            Ok(0) => {
                let _ = write_to_guest(&writer, &Frame::Detach);
                return Ok(());
            }
            Ok(n) => n,
            Err(err) => return Err(err).context("failed to read host terminal stdin"),
        };
        let mut output = Vec::with_capacity(n);
        if filter.push(&buf[..n], &mut output) {
            if !output.is_empty() {
                write_to_guest(&writer, &Frame::Data(output))?;
            }
            write_to_guest(&writer, &Frame::Detach)?;
            return Ok(());
        }
        if !output.is_empty() {
            write_to_guest(&writer, &Frame::Data(output))?;
        }
    }
    Ok(())
}

fn write_to_guest(writer: &Arc<Mutex<UnixStream>>, frame: &Frame) -> Result<()> {
    let mut writer = writer
        .lock()
        .map_err(|_| anyhow!("attach writer lock poisoned"))?;
    write_frame(&mut *writer, frame)
}

enum StdinReadiness {
    Readable,
    TimedOut,
}

fn wait_for_stdin(timeout_ms: i32) -> Result<StdinReadiness> {
    let mut pollfd = libc::pollfd {
        fd: libc::STDIN_FILENO,
        events: libc::POLLIN,
        revents: 0,
    };
    loop {
        let rc = unsafe { libc::poll(&mut pollfd, 1, timeout_ms) };
        if rc > 0 {
            return Ok(StdinReadiness::Readable);
        }
        if rc == 0 {
            return Ok(StdinReadiness::TimedOut);
        }
        let err = std::io::Error::last_os_error();
        if err.kind() == std::io::ErrorKind::Interrupted {
            continue;
        }
        return Err(err).context("failed to poll host terminal stdin");
    }
}

fn terminal_size_changed(last_size: &mut Option<TerminalSize>) -> Option<TerminalSize> {
    size_changed(terminal_size(libc::STDIN_FILENO), last_size)
}

fn size_changed(
    current: Option<TerminalSize>,
    last_size: &mut Option<TerminalSize>,
) -> Option<TerminalSize> {
    if current.is_some() && current != *last_size {
        *last_size = current;
        current
    } else {
        None
    }
}

fn proxy_remote(mut stream: UnixStream) -> Result<AttachOutcome> {
    let mut stdout = std::io::stdout().lock();
    loop {
        match read_frame(&mut stream)? {
            Some(Frame::Data(data)) => {
                stdout.write_all(&data)?;
                stdout.flush()?;
            }
            Some(Frame::Exit { code }) => return Ok(AttachOutcome::Exited(code)),
            Some(Frame::Detach) | None => return Ok(AttachOutcome::Detached),
            Some(Frame::Busy) => bail!("loftd task already has an attached client"),
            Some(Frame::Error(message)) => bail!("loftd attach failed: {message}"),
            Some(frame) => bail!("unexpected loftd attach frame from guest: {frame:?}"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TerminalSize {
    rows: u16,
    cols: u16,
}

fn terminal_size(fd: i32) -> Option<TerminalSize> {
    let mut winsize = libc::winsize {
        ws_row: 0,
        ws_col: 0,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    let rc = unsafe { libc::ioctl(fd, libc::TIOCGWINSZ, &mut winsize) };
    if rc == 0 && winsize.ws_row > 0 && winsize.ws_col > 0 {
        Some(TerminalSize {
            rows: winsize.ws_row,
            cols: winsize.ws_col,
        })
    } else {
        None
    }
}

#[derive(Debug)]
struct RawTerminalMode {
    fd: i32,
    original: libc::termios,
    active: bool,
}

impl RawTerminalMode {
    fn enter(fd: i32) -> Result<Option<Self>> {
        if unsafe { libc::isatty(fd) } != 1 {
            return Ok(None);
        }
        let mut original = unsafe { std::mem::zeroed::<libc::termios>() };
        if unsafe { libc::tcgetattr(fd, &mut original) } != 0 {
            return Err(std::io::Error::last_os_error()).context("failed to read terminal mode");
        }
        let mut raw = original;
        unsafe { libc::cfmakeraw(&mut raw) };
        if unsafe { libc::tcsetattr(fd, libc::TCSANOW, &raw) } != 0 {
            return Err(std::io::Error::last_os_error())
                .context("failed to enter raw terminal mode");
        }
        Ok(Some(Self {
            fd,
            original,
            active: true,
        }))
    }
}

impl Drop for RawTerminalMode {
    fn drop(&mut self) {
        if self.active {
            let _ = unsafe { libc::tcsetattr(self.fd, libc::TCSANOW, &self.original) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use loftd_attach_protocol::DETACH_BYTE;

    #[test]
    fn attach_outcome_messages_are_user_visible() {
        assert_eq!(AttachOutcome::Detached.message(), "loftd: detached\n");
        assert_eq!(
            AttachOutcome::Exited(3).message(),
            "loftd: session exited with status 3\n"
        );
    }

    #[test]
    fn detach_filter_passes_normal_bytes_until_escape() {
        let mut filter = DetachFilter::default();
        let mut output = Vec::new();
        assert!(!filter.push(b"abc", &mut output));
        assert_eq!(output, b"abc");
        assert!(filter.push(&[b'd', DETACH_BYTE, b'e'], &mut output));
        assert_eq!(output, b"abcd");
    }

    #[test]
    fn terminal_size_changed_reports_only_real_changes() {
        let mut previous = Some(TerminalSize { rows: 24, cols: 80 });
        assert_eq!(
            size_changed(Some(TerminalSize { rows: 24, cols: 80 }), &mut previous),
            None
        );
        assert_eq!(previous, Some(TerminalSize { rows: 24, cols: 80 }));
        let changed = Some(TerminalSize {
            rows: 40,
            cols: 120,
        });
        assert_eq!(size_changed(changed, &mut previous), changed);
        assert_eq!(previous, changed);
    }
}
