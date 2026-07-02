use anyhow::{Context, Result, anyhow, bail};
use loftd_attach_protocol::{DetachFilter, Frame, PROTOCOL_VERSION, read_frame, write_frame};
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use super::attach_profile::HostAttachProfiler;

use crate::runtime::session::task_control::{
    ActiveTaskStatus, ProcessInspector, list_records, resolve_task_selector,
};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const CONNECT_RETRY: Duration = Duration::from_millis(50);
const READY_CONNECT_TIMEOUT: Duration = Duration::from_millis(250);
const READY_CONNECT_RETRY: Duration = Duration::from_millis(10);
const IO_BUF_SIZE: usize = 16 * 1024;
const ATTACH_IO_POLL_MS: i32 = 100;
const DAEMON_BOOTSTRAP_TIMEOUT: Duration = Duration::from_secs(10);
const DAEMON_OUTPUT_IDLE_TIMEOUT: Duration = Duration::from_millis(300);

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
    let stream = connect_with_retry(socket_path, ConnectPolicy::reconnect())?;
    attach_stream(stream)
}

pub(crate) fn attach_to_ready_socket(socket_path: &Path, daemon: bool) -> Result<AttachOutcome> {
    attach_to_ready_socket_with_policy(socket_path, ConnectPolicy::post_ready(), daemon)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ConnectPolicy {
    timeout: Duration,
    retry: Duration,
}

impl ConnectPolicy {
    fn reconnect() -> Self {
        Self {
            timeout: CONNECT_TIMEOUT,
            retry: CONNECT_RETRY,
        }
    }

    fn post_ready() -> Self {
        Self {
            timeout: READY_CONNECT_TIMEOUT,
            retry: READY_CONNECT_RETRY,
        }
    }
}

fn connect_with_retry(socket_path: &Path, policy: ConnectPolicy) -> Result<UnixStream> {
    let deadline = Instant::now() + policy.timeout;
    let mut last_error = None;
    while Instant::now() <= deadline {
        match UnixStream::connect(socket_path) {
            Ok(stream) => return Ok(stream),
            Err(err) => {
                last_error = Some(err);
                thread::sleep(policy.retry);
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

fn attach_to_ready_socket_with_policy(
    socket_path: &Path,
    policy: ConnectPolicy,
    daemon: bool,
) -> Result<AttachOutcome> {
    let deadline = Instant::now() + policy.timeout;
    let mut last_error = None;
    while Instant::now() <= deadline {
        match UnixStream::connect(socket_path) {
            Ok(mut stream) => match read_initial_hello(&mut stream)? {
                InitialHello::Ready if daemon => return attach_stream_after_hello_daemon(stream),
                InitialHello::Ready => return attach_stream_after_hello(stream),
                InitialHello::ClosedBeforeHandshake => {
                    last_error = Some(anyhow!("loftd attach socket closed before handshake"));
                }
                InitialHello::Busy => {
                    last_error = Some(anyhow!("loftd task already has an attached client"));
                }
            },
            Err(err) => {
                last_error = Some(anyhow!(err));
            }
        }
        thread::sleep(policy.retry);
    }
    Err(last_error.unwrap_or_else(|| anyhow!("timeout"))).with_context(|| {
        format!(
            "failed to complete initial loftd attach handshake with ready socket '{}'",
            socket_path.display()
        )
    })
}

fn attach_stream(mut stream: UnixStream) -> Result<AttachOutcome> {
    match read_initial_hello(&mut stream)? {
        InitialHello::Ready => attach_stream_after_hello(stream),
        InitialHello::ClosedBeforeHandshake => bail!("loftd attach socket closed before handshake"),
        InitialHello::Busy => bail!("loftd task already has an attached client"),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InitialHello {
    Ready,
    ClosedBeforeHandshake,
    Busy,
}

fn read_initial_hello(stream: &mut UnixStream) -> Result<InitialHello> {
    match read_frame(stream)? {
        Some(Frame::Hello { version }) if version == PROTOCOL_VERSION => Ok(InitialHello::Ready),
        Some(Frame::Hello { version }) => bail!(
            "loftd attach protocol version mismatch: host supports {PROTOCOL_VERSION}, guest reported {version}"
        ),
        Some(Frame::Busy) => Ok(InitialHello::Busy),
        Some(Frame::Error(message)) => bail!("loftd attach failed: {message}"),
        Some(frame) => bail!("unexpected first loftd attach frame: {frame:?}"),
        None => Ok(InitialHello::ClosedBeforeHandshake),
    }
}

fn attach_stream_after_hello(mut stream: UnixStream) -> Result<AttachOutcome> {
    let initial_size = terminal_size(libc::STDIN_FILENO);
    let _raw = RawTerminalMode::enter(libc::STDIN_FILENO)?;
    write_initial_attach_frames(&mut stream, initial_size)?;
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

fn attach_stream_after_hello_daemon(stream: UnixStream) -> Result<AttachOutcome> {
    attach_stream_after_hello_daemon_with_tty_check(stream, || {
        daemon_bootstrap_tty_available(libc::STDIN_FILENO, libc::STDOUT_FILENO)
    })
}

fn attach_stream_after_hello_daemon_with_tty_check(
    mut stream: UnixStream,
    tty_available: impl FnOnce() -> bool,
) -> Result<AttachOutcome> {
    ensure_daemon_bootstrap_tty(tty_available)?;
    let initial_size = terminal_size(libc::STDIN_FILENO);
    let _raw = RawTerminalMode::enter(libc::STDIN_FILENO)?;
    write_initial_attach_frames(&mut stream, initial_size)?;
    let writer = Arc::new(Mutex::new(stream.try_clone()?));
    let active = Arc::new(AtomicBool::new(true));
    let stdin_writer = writer.clone();
    let stdin_active = active.clone();
    let stdin_thread = thread::spawn(move || proxy_stdin(stdin_writer, stdin_active));

    let mut stdout = std::io::stdout().lock();
    let outcome = proxy_remote_until_daemon_idle(
        &mut stream,
        &writer,
        &mut stdout,
        DAEMON_BOOTSTRAP_TIMEOUT,
        DAEMON_OUTPUT_IDLE_TIMEOUT,
    );
    active.store(false, Ordering::Release);
    let _ = stdin_thread.join();
    outcome
}

fn ensure_daemon_bootstrap_tty(tty_available: impl FnOnce() -> bool) -> Result<()> {
    if tty_available() {
        Ok(())
    } else {
        bail!(
            "loftd --daemon requires stdin and stdout to be connected to a TTY for target terminal initialization"
        )
    }
}

fn daemon_bootstrap_tty_available(stdin_fd: i32, stdout_fd: i32) -> bool {
    is_tty(stdin_fd) && is_tty(stdout_fd)
}

fn is_tty(fd: i32) -> bool {
    unsafe { libc::isatty(fd) == 1 }
}

fn write_initial_attach_frames<W>(writer: &mut W, initial_size: Option<TerminalSize>) -> Result<()>
where
    W: Write,
{
    if let Some(size) = initial_size {
        write_frame(
            writer,
            &Frame::Resize {
                rows: size.rows,
                cols: size.cols,
            },
        )?;
    }
    write_frame(writer, &Frame::Attach)
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
            StdinReadiness::TimedOut => {
                let mut output = Vec::new();
                filter.flush_incomplete_escape_sequence(&mut output);
                if !output.is_empty() {
                    write_to_guest(&writer, &Frame::Data(output))?;
                }
                continue;
            }
            StdinReadiness::Readable => {}
        }
        let n = match input.read(&mut buf) {
            Ok(0) => {
                let mut output = Vec::new();
                filter.flush_pending(&mut output);
                if !output.is_empty() {
                    let _ = write_to_guest(&writer, &Frame::Data(output));
                }
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
    let mut stderr = std::io::stderr().lock();
    proxy_remote_with_profile(
        &mut stream,
        &mut stdout,
        &mut stderr,
        HostAttachProfiler::from_process_env(),
    )
}

fn proxy_remote_with_profile<R, W, E>(
    stream: &mut R,
    stdout: &mut W,
    stderr: &mut E,
    mut profiler: HostAttachProfiler,
) -> Result<AttachOutcome>
where
    R: Read,
    W: Write,
    E: Write,
{
    let result = proxy_remote_loop(stream, stdout, &mut profiler);
    let report = profiler.report_to(stderr);
    if let Err(err) = report {
        return Err(err).context("failed to write loftd attach profile report");
    }
    result
}

fn proxy_remote_loop<R, W>(
    stream: &mut R,
    stdout: &mut W,
    profiler: &mut HostAttachProfiler,
) -> Result<AttachOutcome>
where
    R: Read,
    W: Write,
{
    loop {
        let frame = if profiler.is_enabled() {
            let started = Instant::now();
            let frame = read_frame(stream);
            profiler.record_frame_read(started.elapsed());
            frame?
        } else {
            read_frame(stream)?
        };
        match frame {
            Some(Frame::Data(data)) => {
                profiler.record_data_frame(data.len());
                if profiler.is_enabled() {
                    let started = Instant::now();
                    stdout.write_all(&data)?;
                    profiler.record_stdout_write(started.elapsed());
                    let started = Instant::now();
                    stdout.flush()?;
                    profiler.record_stdout_flush(started.elapsed());
                } else {
                    stdout.write_all(&data)?;
                    stdout.flush()?;
                }
            }
            Some(Frame::Exit { code }) => return Ok(AttachOutcome::Exited(code)),
            Some(Frame::Detach) | None => return Ok(AttachOutcome::Detached),
            Some(Frame::Busy) => bail!("loftd task already has an attached client"),
            Some(Frame::Error(message)) => bail!("loftd attach failed: {message}"),
            Some(frame) => bail!("unexpected loftd attach frame from guest: {frame:?}"),
        }
    }
}

fn proxy_remote_until_daemon_idle<W: Write>(
    stream: &mut UnixStream,
    writer: &Arc<Mutex<UnixStream>>,
    stdout: &mut W,
    startup_timeout: Duration,
    idle_timeout: Duration,
) -> Result<AttachOutcome> {
    let mut stderr = std::io::stderr().lock();
    proxy_remote_until_daemon_idle_with_profile(
        stream,
        writer,
        stdout,
        &mut stderr,
        startup_timeout,
        idle_timeout,
        HostAttachProfiler::from_process_env(),
    )
}

fn proxy_remote_until_daemon_idle_with_profile<W: Write, E: Write>(
    stream: &mut UnixStream,
    writer: &Arc<Mutex<UnixStream>>,
    stdout: &mut W,
    stderr: &mut E,
    startup_timeout: Duration,
    idle_timeout: Duration,
    mut profiler: HostAttachProfiler,
) -> Result<AttachOutcome> {
    let result = proxy_remote_until_daemon_idle_loop(
        stream,
        writer,
        stdout,
        startup_timeout,
        idle_timeout,
        &mut profiler,
    );
    let report = profiler.report_to(stderr);
    if let Err(err) = report {
        return Err(err).context("failed to write loftd attach profile report");
    }
    result
}

fn proxy_remote_until_daemon_idle_loop<W: Write>(
    stream: &mut UnixStream,
    writer: &Arc<Mutex<UnixStream>>,
    stdout: &mut W,
    startup_timeout: Duration,
    idle_timeout: Duration,
    profiler: &mut HostAttachProfiler,
) -> Result<AttachOutcome> {
    let startup_deadline = Instant::now() + startup_timeout;
    let mut saw_output = false;
    loop {
        let timeout = if saw_output {
            idle_timeout
        } else {
            startup_deadline
                .checked_duration_since(Instant::now())
                .unwrap_or_default()
        };
        match wait_for_remote_frame(stream.as_raw_fd(), timeout)? {
            RemoteReadiness::TimedOut if saw_output => {
                write_to_guest(writer, &Frame::Detach)?;
                return Ok(AttachOutcome::Detached);
            }
            RemoteReadiness::TimedOut => {
                bail!(
                    "timed out waiting for initial output from loftd --daemon bootstrap before detach"
                );
            }
            RemoteReadiness::Readable => {}
        }
        let frame = if profiler.is_enabled() {
            let started = Instant::now();
            let frame = read_frame(stream);
            profiler.record_frame_read(started.elapsed());
            frame?
        } else {
            read_frame(stream)?
        };
        match frame {
            Some(Frame::Data(data)) => {
                profiler.record_data_frame(data.len());
                if profiler.is_enabled() {
                    let started = Instant::now();
                    stdout.write_all(&data)?;
                    profiler.record_stdout_write(started.elapsed());
                    let started = Instant::now();
                    stdout.flush()?;
                    profiler.record_stdout_flush(started.elapsed());
                } else {
                    stdout.write_all(&data)?;
                    stdout.flush()?;
                }
                saw_output = true;
            }
            Some(Frame::Exit { code }) => return Ok(AttachOutcome::Exited(code)),
            Some(Frame::Detach) | None => return Ok(AttachOutcome::Detached),
            Some(Frame::Busy) => bail!("loftd task already has an attached client"),
            Some(Frame::Error(message)) => bail!("loftd attach failed: {message}"),
            Some(frame) => bail!("unexpected loftd attach frame from guest: {frame:?}"),
        }
    }
}

enum RemoteReadiness {
    Readable,
    TimedOut,
}

fn wait_for_remote_frame(fd: i32, timeout: Duration) -> Result<RemoteReadiness> {
    let timeout_ms = i32::try_from(timeout.as_millis()).unwrap_or(i32::MAX);
    let mut pollfd = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };
    loop {
        let rc = unsafe { libc::poll(&mut pollfd, 1, timeout_ms) };
        if rc > 0 {
            return Ok(RemoteReadiness::Readable);
        }
        if rc == 0 {
            return Ok(RemoteReadiness::TimedOut);
        }
        let err = std::io::Error::last_os_error();
        if err.kind() == std::io::ErrorKind::Interrupted {
            continue;
        }
        return Err(err).context("failed to poll loftd attach stream");
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
        if !is_tty(fd) {
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
    use loftd_attach_protocol::{DETACH_PREFIX_BYTE, DETACH_SUFFIX_BYTE};
    use std::os::unix::net::UnixListener;

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
        assert!(filter.push(
            &[b'd', DETACH_PREFIX_BYTE, DETACH_SUFFIX_BYTE, b'e'],
            &mut output
        ));
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

    #[test]
    fn post_ready_connect_policy_is_tiny_and_distinct_from_reconnect() {
        let post_ready = ConnectPolicy::post_ready();
        let reconnect = ConnectPolicy::reconnect();

        assert!(post_ready.timeout <= Duration::from_millis(250));
        assert!(post_ready.retry <= Duration::from_millis(25));
        assert!(post_ready.timeout < reconnect.timeout);
        assert!(post_ready.retry < reconnect.retry);
    }

    #[test]
    fn post_ready_attach_retries_socket_close_before_handshake() {
        let temp = tempfile::tempdir().unwrap();
        let socket = temp.path().join("attach.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let server = thread::spawn(move || {
            let (first, _) = listener.accept().unwrap();
            drop(first);

            let (mut client, _) = listener.accept().unwrap();
            write_frame(
                &mut client,
                &Frame::Hello {
                    version: PROTOCOL_VERSION,
                },
            )
            .unwrap();
            assert_eq!(read_frame(&mut client).unwrap(), Some(Frame::Attach));
            write_frame(&mut client, &Frame::Exit { code: 7 }).unwrap();
        });

        let outcome = attach_to_ready_socket(&socket, false).unwrap();

        assert_eq!(outcome, AttachOutcome::Exited(7));
        server.join().unwrap();
    }

    #[test]
    fn initial_attach_frames_send_resize_before_command_start_signal() {
        let mut bytes = Vec::new();
        write_initial_attach_frames(
            &mut bytes,
            Some(TerminalSize {
                rows: 30,
                cols: 100,
            }),
        )
        .unwrap();
        let mut cursor = std::io::Cursor::new(bytes);

        assert_eq!(
            read_frame(&mut cursor).unwrap(),
            Some(Frame::Resize {
                rows: 30,
                cols: 100
            })
        );
        assert_eq!(read_frame(&mut cursor).unwrap(), Some(Frame::Attach));
    }

    #[test]
    fn initial_attach_frames_send_attach_when_resize_unknown() {
        let mut bytes = Vec::new();
        write_initial_attach_frames(&mut bytes, None).unwrap();
        let mut cursor = std::io::Cursor::new(bytes);

        assert_eq!(read_frame(&mut cursor).unwrap(), Some(Frame::Attach));
    }

    #[test]
    fn daemon_bootstrap_requires_tty_fds() {
        let file = tempfile::tempfile().unwrap();

        assert!(!daemon_bootstrap_tty_available(
            file.as_raw_fd(),
            file.as_raw_fd()
        ));
    }

    #[test]
    fn daemon_tty_failure_closes_without_sending_attach() {
        let (client, mut server) = UnixStream::pair().unwrap();
        let server_thread = thread::spawn(move || {
            assert_eq!(read_frame(&mut server).unwrap(), None);
        });

        let err = attach_stream_after_hello_daemon_with_tty_check(client, || false)
            .expect_err("daemon attach should fail when TTY bootstrap is unavailable");

        assert!(
            format!("{err:#}").contains("requires stdin and stdout"),
            "unexpected error: {err:#}"
        );
        server_thread.join().unwrap();
    }

    #[test]
    fn daemon_remote_proxy_detaches_after_first_output_idle() {
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let writer = Arc::new(Mutex::new(client.try_clone().unwrap()));
        let server_thread = thread::spawn(move || {
            write_frame(&mut server, &Frame::Data(b"fish> ".to_vec())).unwrap();
            assert_eq!(read_frame(&mut server).unwrap(), Some(Frame::Detach));
        });
        let mut output = Vec::new();

        let outcome = proxy_remote_until_daemon_idle(
            &mut client,
            &writer,
            &mut output,
            Duration::from_secs(1),
            Duration::from_millis(20),
        )
        .unwrap();

        assert_eq!(outcome, AttachOutcome::Detached);
        assert_eq!(output, b"fish> ");
        server_thread.join().unwrap();
    }

    #[test]
    fn daemon_remote_proxy_returns_exit_before_idle_detach() {
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let writer = Arc::new(Mutex::new(client.try_clone().unwrap()));
        let server_thread = thread::spawn(move || {
            write_frame(&mut server, &Frame::Data(b"booting".to_vec())).unwrap();
            write_frame(&mut server, &Frame::Exit { code: 9 }).unwrap();
        });
        let mut output = Vec::new();

        let outcome = proxy_remote_until_daemon_idle(
            &mut client,
            &writer,
            &mut output,
            Duration::from_secs(1),
            Duration::from_millis(200),
        )
        .unwrap();

        assert_eq!(outcome, AttachOutcome::Exited(9));
        assert_eq!(output, b"booting");
        server_thread.join().unwrap();
    }

    #[test]
    fn daemon_remote_proxy_times_out_without_initial_output() {
        let (mut client, _server) = UnixStream::pair().unwrap();
        let writer = Arc::new(Mutex::new(client.try_clone().unwrap()));
        let mut output = Vec::new();

        let err = proxy_remote_until_daemon_idle(
            &mut client,
            &writer,
            &mut output,
            Duration::from_millis(20),
            Duration::from_millis(20),
        )
        .expect_err("daemon bootstrap should require initial output before detach");

        assert!(
            format!("{err:#}").contains("timed out waiting for initial output"),
            "unexpected error: {err:#}"
        );
        assert!(output.is_empty());
    }

    #[test]
    fn post_ready_attach_retries_transient_busy_from_readiness_probe_teardown() {
        let temp = tempfile::tempdir().unwrap();
        let socket = temp.path().join("attach.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let server = thread::spawn(move || {
            let (mut first, _) = listener.accept().unwrap();
            write_frame(&mut first, &Frame::Busy).unwrap();

            let (mut client, _) = listener.accept().unwrap();
            write_frame(
                &mut client,
                &Frame::Hello {
                    version: PROTOCOL_VERSION,
                },
            )
            .unwrap();
            assert_eq!(read_frame(&mut client).unwrap(), Some(Frame::Attach));
            write_frame(&mut client, &Frame::Exit { code: 0 }).unwrap();
        });

        let outcome = attach_to_ready_socket(&socket, false).unwrap();

        assert_eq!(outcome, AttachOutcome::Exited(0));
        server.join().unwrap();
    }

    #[test]
    fn attach_profile_proxy_preserves_stdout_and_reports_to_stderr() {
        let mut stream = Vec::new();
        write_frame(&mut stream, &Frame::Data(b"hello".to_vec())).unwrap();
        write_frame(&mut stream, &Frame::Detach).unwrap();
        let mut stream = std::io::Cursor::new(stream);
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let outcome = proxy_remote_with_profile(
            &mut stream,
            &mut stdout,
            &mut stderr,
            HostAttachProfiler::new(true),
        )
        .unwrap();

        assert_eq!(outcome, AttachOutcome::Detached);
        assert_eq!(stdout, b"hello");
        let stderr = String::from_utf8(stderr).unwrap();
        assert!(stderr.starts_with("loftd attach profile role=host "));
        assert!(stderr.contains("frames=1"));
        assert!(stderr.contains("bytes=5"));
        assert!(stderr.contains("frame_max_bytes=5"));
        assert!(stderr.contains("frame_avg_bytes=5"));
        assert!(stderr.contains("stdout_write_total_us="));
        assert!(stderr.contains("stdout_flush_total_us="));
    }

    #[test]
    fn attach_profile_proxy_is_quiet_when_disabled() {
        let mut stream = Vec::new();
        write_frame(&mut stream, &Frame::Data(b"hello".to_vec())).unwrap();
        write_frame(&mut stream, &Frame::Detach).unwrap();
        let mut stream = std::io::Cursor::new(stream);
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let outcome = proxy_remote_with_profile(
            &mut stream,
            &mut stdout,
            &mut stderr,
            HostAttachProfiler::new(false),
        )
        .unwrap();

        assert_eq!(outcome, AttachOutcome::Detached);
        assert_eq!(stdout, b"hello");
        assert!(stderr.is_empty());
    }

    #[test]
    fn daemon_attach_profile_reports_output_path() {
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let writer = Arc::new(Mutex::new(client.try_clone().unwrap()));
        let server_thread = thread::spawn(move || {
            write_frame(&mut server, &Frame::Data(b"ready".to_vec())).unwrap();
            assert_eq!(read_frame(&mut server).unwrap(), Some(Frame::Detach));
        });
        let mut output = Vec::new();
        let mut stderr = Vec::new();

        let outcome = proxy_remote_until_daemon_idle_with_profile(
            &mut client,
            &writer,
            &mut output,
            &mut stderr,
            Duration::from_secs(1),
            Duration::from_millis(20),
            HostAttachProfiler::new(true),
        )
        .unwrap();

        assert_eq!(outcome, AttachOutcome::Detached);
        assert_eq!(output, b"ready");
        let stderr = String::from_utf8(stderr).unwrap();
        assert!(stderr.contains("role=host"));
        assert!(stderr.contains("frame_max_bytes=5"));
        server_thread.join().unwrap();
    }
}
