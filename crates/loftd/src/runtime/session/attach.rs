use anyhow::{Context, Result, anyhow, bail};
use loftd_attach_protocol::{
    DetachFilter, Frame, PROTOCOL_VERSION, read_frame,
    terminal_trace::{trace_data_from_env, trace_event_from_env},
    write_frame,
};
use std::io::{ErrorKind, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::runtime::session::attach_profile::HostAttachProfiler;

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
const FRAME_HEADER_LEN: usize = 5;
const MAX_ATTACH_PAYLOAD_LEN: usize = 16 * 1024 * 1024;
const HOST_OUTPUT_BATCH_MAX_FRAMES: usize = 16;
const HOST_OUTPUT_BATCH_MAX_BYTES: usize = IO_BUF_SIZE;

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
        trace_event_from_env(
            "host",
            "resize",
            &format!(
                "direction=host-to-guest-initial rows={} cols={}",
                size.rows, size.cols
            ),
        );
        write_frame(
            writer,
            &Frame::Resize {
                rows: size.rows,
                cols: size.cols,
            },
        )?;
    }
    trace_event_from_env("host", "attach", "direction=host-to-guest");
    write_frame(writer, &Frame::Attach)
}

fn proxy_stdin(writer: Arc<Mutex<UnixStream>>, active: Arc<AtomicBool>) -> Result<()> {
    let mut filter = DetachFilter::default();
    let mut input = std::io::stdin().lock();
    let mut buf = [0u8; IO_BUF_SIZE];
    let mut last_size = terminal_size(libc::STDIN_FILENO);
    while active.load(Ordering::Acquire) {
        if let Some(size) = terminal_size_changed(&mut last_size) {
            trace_event_from_env(
                "host",
                "resize",
                &format!(
                    "direction=host-to-guest-live rows={} cols={}",
                    size.rows, size.cols
                ),
            );
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
                    trace_data_from_env("host", "host-to-guest-stdin-flush", &output);
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
                    trace_data_from_env("host", "host-to-guest-stdin-eof", &output);
                    let _ = write_to_guest(&writer, &Frame::Data(output));
                }
                trace_event_from_env("host", "detach", "direction=host-to-guest reason=stdin-eof");
                let _ = write_to_guest(&writer, &Frame::Detach);
                return Ok(());
            }
            Ok(n) => n,
            Err(err) => return Err(err).context("failed to read host terminal stdin"),
        };
        let mut output = Vec::with_capacity(n);
        if filter.push(&buf[..n], &mut output) {
            if !output.is_empty() {
                trace_data_from_env("host", "host-to-guest-stdin-detach", &output);
                write_to_guest(&writer, &Frame::Data(output))?;
            }
            trace_event_from_env(
                "host",
                "detach",
                "direction=host-to-guest reason=detach-key",
            );
            write_to_guest(&writer, &Frame::Detach)?;
            return Ok(());
        }
        if !output.is_empty() {
            trace_data_from_env("host", "host-to-guest-stdin", &output);
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
    proxy_remote_unix_with_profile(
        &mut stream,
        &mut stdout,
        &mut stderr,
        HostAttachProfiler::from_process_env(),
    )
}

fn proxy_remote_unix_with_profile<W, E>(
    stream: &mut UnixStream,
    stdout: &mut W,
    stderr: &mut E,
    mut profiler: HostAttachProfiler,
) -> Result<AttachOutcome>
where
    W: Write,
    E: Write,
{
    let mut source = UnixRemoteFrameReader::new(stream);
    let result = proxy_remote_loop(&mut source, stdout, &mut profiler);
    let report = profiler.report_to(stderr);
    if let Err(err) = report {
        return Err(err).context("failed to write loftd attach profile report");
    }
    result
}

#[cfg(test)]
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
    let mut source = BlockingRemoteFrameReader::new(stream);
    let result = proxy_remote_loop(&mut source, stdout, &mut profiler);
    let report = profiler.report_to(stderr);
    if let Err(err) = report {
        return Err(err).context("failed to write loftd attach profile report");
    }
    result
}

fn proxy_remote_loop<S, W>(
    source: &mut S,
    stdout: &mut W,
    profiler: &mut HostAttachProfiler,
) -> Result<AttachOutcome>
where
    S: RemoteFrameSource,
    W: Write,
{
    loop {
        let frame = source.read_frame(profiler)?;
        match frame {
            Some(Frame::Data(data)) => {
                trace_data_from_env("host", "guest-to-host-stdout", &data);
                let mut batch = StdoutBatch::new();
                batch.push_data(data, profiler);
                loop {
                    if batch.is_full() {
                        batch.flush_to(stdout, profiler)?;
                        break;
                    }
                    match source.try_read_frame(profiler)? {
                        TryRemoteFrame::Frame(Frame::Data(data)) => {
                            trace_data_from_env("host", "guest-to-host-stdout", &data);
                            if !batch.can_accept(data.len()) {
                                batch.flush_to(stdout, profiler)?;
                            }
                            batch.push_data(data, profiler);
                        }
                        TryRemoteFrame::Frame(frame) => {
                            batch.flush_to(stdout, profiler)?;
                            return handle_remote_control_frame(frame);
                        }
                        TryRemoteFrame::NotReady => {
                            batch.flush_to(stdout, profiler)?;
                            break;
                        }
                        TryRemoteFrame::Eof => {
                            batch.flush_to(stdout, profiler)?;
                            return Ok(AttachOutcome::Detached);
                        }
                    }
                }
            }
            Some(frame) => return handle_remote_control_frame(frame),
            None => return Ok(AttachOutcome::Detached),
        }
    }
}

trait RemoteFrameSource {
    fn read_frame(&mut self, profiler: &mut HostAttachProfiler) -> Result<Option<Frame>>;
    fn try_read_frame(&mut self, profiler: &mut HostAttachProfiler) -> Result<TryRemoteFrame>;
}

enum TryRemoteFrame {
    Frame(Frame),
    NotReady,
    Eof,
}

#[cfg(test)]
struct BlockingRemoteFrameReader<'a, R> {
    stream: &'a mut R,
}

#[cfg(test)]
impl<'a, R> BlockingRemoteFrameReader<'a, R> {
    fn new(stream: &'a mut R) -> Self {
        Self { stream }
    }
}

#[cfg(test)]
impl<R: Read> RemoteFrameSource for BlockingRemoteFrameReader<'_, R> {
    fn read_frame(&mut self, profiler: &mut HostAttachProfiler) -> Result<Option<Frame>> {
        read_profiled_frame(self.stream, profiler)
    }

    fn try_read_frame(&mut self, _profiler: &mut HostAttachProfiler) -> Result<TryRemoteFrame> {
        Ok(TryRemoteFrame::NotReady)
    }
}

struct UnixRemoteFrameReader<'a> {
    stream: &'a mut UnixStream,
    buffer: Vec<u8>,
}

impl<'a> UnixRemoteFrameReader<'a> {
    fn new(stream: &'a mut UnixStream) -> Self {
        Self {
            stream,
            buffer: Vec::new(),
        }
    }

    fn read_frame_blocking(&mut self) -> Result<Option<Frame>> {
        let mut scratch = [0u8; IO_BUF_SIZE];
        loop {
            if let Some(frame) = self.decode_buffered_frame()? {
                return Ok(Some(frame));
            }
            match self.stream.read(&mut scratch) {
                Ok(0) if self.buffer.is_empty() => return Ok(None),
                Ok(0) => bail!("truncated loftd attach protocol frame"),
                Ok(n) => self.buffer.extend_from_slice(&scratch[..n]),
                Err(err) if err.kind() == ErrorKind::Interrupted => continue,
                Err(err) => return Err(err).context("failed to read loftd attach stream"),
            }
        }
    }

    fn try_read_frame_now(&mut self) -> Result<TryRemoteFrame> {
        let mut scratch = [0u8; IO_BUF_SIZE];
        loop {
            if let Some(frame) = self.decode_buffered_frame()? {
                return Ok(TryRemoteFrame::Frame(frame));
            }
            let rc = unsafe {
                libc::recv(
                    self.stream.as_raw_fd(),
                    scratch.as_mut_ptr().cast(),
                    scratch.len(),
                    libc::MSG_DONTWAIT,
                )
            };
            if rc > 0 {
                let n = usize::try_from(rc).expect("positive recv result fits usize");
                self.buffer.extend_from_slice(&scratch[..n]);
                continue;
            }
            if rc == 0 {
                if self.buffer.is_empty() {
                    return Ok(TryRemoteFrame::Eof);
                }
                bail!("truncated loftd attach protocol frame");
            }
            let err = std::io::Error::last_os_error();
            match err.kind() {
                ErrorKind::WouldBlock => return Ok(TryRemoteFrame::NotReady),
                ErrorKind::Interrupted => continue,
                _ => return Err(err).context("failed to drain loftd attach stream"),
            }
        }
    }

    fn decode_buffered_frame(&mut self) -> Result<Option<Frame>> {
        if self.buffer.len() < FRAME_HEADER_LEN {
            return Ok(None);
        }
        let payload_len = u32::from_be_bytes([
            self.buffer[1],
            self.buffer[2],
            self.buffer[3],
            self.buffer[4],
        ]) as usize;
        if payload_len > MAX_ATTACH_PAYLOAD_LEN {
            bail!("loftd attach protocol frame payload exceeds maximum length");
        }
        let frame_len = FRAME_HEADER_LEN + payload_len;
        if self.buffer.len() < frame_len {
            return Ok(None);
        }
        let frame_bytes: Vec<u8> = self.buffer.drain(..frame_len).collect();
        let mut cursor = std::io::Cursor::new(frame_bytes);
        let frame = read_frame(&mut cursor)?
            .ok_or_else(|| anyhow!("buffered frame decoded as clean EOF"))?;
        Ok(Some(frame))
    }
}

impl RemoteFrameSource for UnixRemoteFrameReader<'_> {
    fn read_frame(&mut self, profiler: &mut HostAttachProfiler) -> Result<Option<Frame>> {
        if profiler.is_enabled() {
            let started = Instant::now();
            let frame = self.read_frame_blocking();
            profiler.record_frame_read(started.elapsed());
            frame
        } else {
            self.read_frame_blocking()
        }
    }

    fn try_read_frame(&mut self, profiler: &mut HostAttachProfiler) -> Result<TryRemoteFrame> {
        if profiler.is_enabled() {
            let started = Instant::now();
            let frame = self.try_read_frame_now();
            if matches!(
                frame,
                Ok(TryRemoteFrame::Frame(_)) | Ok(TryRemoteFrame::Eof)
            ) {
                profiler.record_frame_read(started.elapsed());
            }
            frame
        } else {
            self.try_read_frame_now()
        }
    }
}

fn read_profiled_frame<R: Read>(
    stream: &mut R,
    profiler: &mut HostAttachProfiler,
) -> Result<Option<Frame>> {
    if profiler.is_enabled() {
        let started = Instant::now();
        let frame = read_frame(stream);
        profiler.record_frame_read(started.elapsed());
        frame
    } else {
        read_frame(stream)
    }
}

struct StdoutBatch {
    bytes: Vec<u8>,
    frames: usize,
}

impl StdoutBatch {
    fn new() -> Self {
        Self {
            bytes: Vec::new(),
            frames: 0,
        }
    }

    fn can_accept(&self, byte_count: usize) -> bool {
        self.frames == 0
            || (self.frames < HOST_OUTPUT_BATCH_MAX_FRAMES
                && self.bytes.len().saturating_add(byte_count) <= HOST_OUTPUT_BATCH_MAX_BYTES)
    }

    fn is_full(&self) -> bool {
        self.frames >= HOST_OUTPUT_BATCH_MAX_FRAMES
            || self.bytes.len() >= HOST_OUTPUT_BATCH_MAX_BYTES
    }

    fn push_data(&mut self, data: Vec<u8>, profiler: &mut HostAttachProfiler) {
        profiler.record_data_frame(data.len());
        self.frames += 1;
        self.bytes.extend(data);
    }

    fn flush_to<W: Write>(
        &mut self,
        stdout: &mut W,
        profiler: &mut HostAttachProfiler,
    ) -> Result<()> {
        if self.bytes.is_empty() {
            return Ok(());
        }
        let byte_count = self.bytes.len();
        let frame_count = self.frames;
        if profiler.is_enabled() {
            let started = Instant::now();
            stdout.write_all(&self.bytes)?;
            profiler.record_stdout_write(started.elapsed());
            let started = Instant::now();
            stdout.flush()?;
            profiler.record_stdout_flush(started.elapsed());
        } else {
            stdout.write_all(&self.bytes)?;
            stdout.flush()?;
        }
        profiler.record_stdout_batch(frame_count, byte_count);
        self.bytes.clear();
        self.frames = 0;
        Ok(())
    }
}

fn handle_remote_control_frame(frame: Frame) -> Result<AttachOutcome> {
    match frame {
        Frame::Exit { code } => Ok(AttachOutcome::Exited(code)),
        Frame::Detach => Ok(AttachOutcome::Detached),
        Frame::Busy => bail!("loftd task already has an attached client"),
        Frame::Error(message) => bail!("loftd attach failed: {message}"),
        frame => bail!("unexpected loftd attach frame from guest: {frame:?}"),
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
        let frame = read_profiled_frame(stream, profiler)?;
        match frame {
            Some(Frame::Data(data)) => {
                trace_data_from_env("host", "guest-to-host-stdout", &data);
                let mut batch = StdoutBatch::new();
                batch.push_data(data, profiler);
                batch.flush_to(stdout, profiler)?;
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
    use std::sync::mpsc;

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
        assert!(stderr.contains("stdout_batches=1"));
        assert!(stderr.contains("stdout_batch_frames_max=1"));
        assert!(stderr.contains("stdout_batch_bytes_max=5"));
        assert!(stderr.contains("stdout_batch_frames_avg=1"));
        assert!(stderr.contains("stdout_write_count=1"));
        assert!(stderr.contains("stdout_write_total_us="));
        assert!(stderr.contains("stdout_flush_count=1"));
        assert!(stderr.contains("stdout_flush_total_us="));
    }

    #[derive(Default)]
    struct CountingWriter {
        bytes: Vec<u8>,
        writes: usize,
        flushes: usize,
    }

    impl Write for CountingWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.writes += 1;
            self.bytes.extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            self.flushes += 1;
            Ok(())
        }
    }

    fn encoded_frame(frame: &Frame) -> Vec<u8> {
        let mut bytes = Vec::new();
        write_frame(&mut bytes, frame).unwrap();
        bytes
    }

    #[test]
    fn unix_remote_proxy_batches_consecutive_ready_data_frames() {
        let (mut client, mut server) = UnixStream::pair().unwrap();
        write_frame(&mut server, &Frame::Data(b"ab".to_vec())).unwrap();
        write_frame(&mut server, &Frame::Data(b"cd".to_vec())).unwrap();
        write_frame(&mut server, &Frame::Data(b"ef".to_vec())).unwrap();
        write_frame(&mut server, &Frame::Detach).unwrap();
        let mut stdout = CountingWriter::default();
        let mut stderr = Vec::new();

        let outcome = proxy_remote_unix_with_profile(
            &mut client,
            &mut stdout,
            &mut stderr,
            HostAttachProfiler::new(true),
        )
        .unwrap();

        assert_eq!(outcome, AttachOutcome::Detached);
        assert_eq!(stdout.bytes, b"abcdef");
        assert_eq!(stdout.writes, 1);
        assert_eq!(stdout.flushes, 1);
        let stderr = String::from_utf8(stderr).unwrap();
        assert!(stderr.contains("frames=3"));
        assert!(stderr.contains("bytes=6"));
        assert!(stderr.contains("stdout_batches=1"));
        assert!(stderr.contains("stdout_batch_frames_max=3"));
        assert!(stderr.contains("stdout_batch_bytes_max=6"));
        assert!(stderr.contains("stdout_batch_frames_avg=3"));
        assert!(stderr.contains("stdout_write_count=1"));
        assert!(stderr.contains("stdout_flush_count=1"));
    }

    struct NotifyingWriter {
        bytes: Vec<u8>,
        first_flush: Option<mpsc::Sender<()>>,
    }

    impl NotifyingWriter {
        fn new(first_flush: mpsc::Sender<()>) -> Self {
            Self {
                bytes: Vec::new(),
                first_flush: Some(first_flush),
            }
        }
    }

    impl Write for NotifyingWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.bytes.extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            if self.bytes == b"first"
                && let Some(tx) = self.first_flush.take()
            {
                tx.send(()).unwrap();
            }
            Ok(())
        }
    }

    #[test]
    fn unix_remote_proxy_flushes_before_waiting_for_partial_next_frame() {
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let first = encoded_frame(&Frame::Data(b"first".to_vec()));
        let second = encoded_frame(&Frame::Data(b"second".to_vec()));
        let detach = encoded_frame(&Frame::Detach);
        let (flushed_tx, flushed_rx) = mpsc::channel();
        let server_thread = thread::spawn(move || {
            server.write_all(&first).unwrap();
            server.write_all(&second[..3]).unwrap();
            flushed_rx
                .recv_timeout(Duration::from_secs(1))
                .expect("first frame should flush before partial second frame completes");
            server.write_all(&second[3..]).unwrap();
            server.write_all(&detach).unwrap();
        });
        let mut stdout = NotifyingWriter::new(flushed_tx);
        let mut stderr = Vec::new();

        let outcome = proxy_remote_unix_with_profile(
            &mut client,
            &mut stdout,
            &mut stderr,
            HostAttachProfiler::new(true),
        )
        .unwrap();

        assert_eq!(outcome, AttachOutcome::Detached);
        assert_eq!(stdout.bytes, b"firstsecond");
        let stderr = String::from_utf8(stderr).unwrap();
        assert!(stderr.contains("frames=2"));
        assert!(stderr.contains("stdout_batches=2"));
        server_thread.join().unwrap();
    }

    #[test]
    fn unix_remote_proxy_does_not_set_nonblocking_on_shared_socket() {
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let mut stdin_writer_clone = client.try_clone().unwrap();
        assert!(!fd_is_nonblocking(client.as_raw_fd()));
        assert!(!fd_is_nonblocking(stdin_writer_clone.as_raw_fd()));
        let server_thread = thread::spawn(move || {
            write_frame(&mut server, &Frame::Data(b"ready".to_vec())).unwrap();
            write_frame(&mut server, &Frame::Detach).unwrap();
            assert_eq!(read_frame(&mut server).unwrap(), Some(Frame::Detach));
        });
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let outcome = proxy_remote_unix_with_profile(
            &mut client,
            &mut stdout,
            &mut stderr,
            HostAttachProfiler::new(true),
        )
        .unwrap();

        assert_eq!(outcome, AttachOutcome::Detached);
        assert_eq!(stdout, b"ready");
        assert!(!fd_is_nonblocking(client.as_raw_fd()));
        assert!(!fd_is_nonblocking(stdin_writer_clone.as_raw_fd()));
        write_frame(&mut stdin_writer_clone, &Frame::Detach).unwrap();
        server_thread.join().unwrap();
    }

    fn fd_is_nonblocking(fd: i32) -> bool {
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        assert!(flags >= 0, "F_GETFL failed");
        flags & libc::O_NONBLOCK != 0
    }

    #[test]
    fn unix_remote_proxy_flushes_data_before_exit() {
        let (mut client, mut server) = UnixStream::pair().unwrap();
        write_frame(&mut server, &Frame::Data(b"booting".to_vec())).unwrap();
        write_frame(&mut server, &Frame::Exit { code: 17 }).unwrap();
        let mut stdout = CountingWriter::default();
        let mut stderr = Vec::new();

        let outcome = proxy_remote_unix_with_profile(
            &mut client,
            &mut stdout,
            &mut stderr,
            HostAttachProfiler::new(true),
        )
        .unwrap();

        assert_eq!(outcome, AttachOutcome::Exited(17));
        assert_eq!(stdout.bytes, b"booting");
        assert_eq!(stdout.writes, 1);
        assert_eq!(stdout.flushes, 1);
        let stderr = String::from_utf8(stderr).unwrap();
        assert!(stderr.contains("stdout_batches=1"));
        assert!(stderr.contains("stdout_write_count=1"));
        assert!(stderr.contains("stdout_flush_count=1"));
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
        assert!(stderr.contains("stdout_batches=1"));
        assert!(stderr.contains("stdout_write_count=1"));
        assert!(stderr.contains("stdout_flush_count=1"));
        server_thread.join().unwrap();
    }
}
