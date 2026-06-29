use anyhow::{Context, Result, bail};
use loftd_attach_protocol::{Frame, PROTOCOL_VERSION, read_frame, write_frame};
use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, RawFd};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::thread;
use std::time::Duration;

use crate::guest_init::components::home::identity::DevIdentity;
use crate::guest_init::process;

const IO_BUF_SIZE: usize = 16 * 1024;
const POLL_TIMEOUT_MS: i32 = 100;
const DEFAULT_TERMINAL_ROWS: u16 = 24;
const DEFAULT_TERMINAL_COLS: u16 = 80;
const TERMINAL_SCROLLBACK_ROWS: usize = 2_000;
const ENTER_ALTERNATE_SCREEN: &[u8] = b"\x1b[?1049h";
const EXIT_ALTERNATE_SCREEN: &[u8] = b"\x1b[?1049l";
// Reset attributes, home the cursor, and clear the visible viewport before
// replaying the tracked terminal state so stale host-terminal cells disappear.
const CLEAR_VISIBLE_SCREEN: &[u8] = b"\x1b[m\x1b[H\x1b[J";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::guest_init) struct ManagedSessionConfig {
    pub(in crate::guest_init) port: u32,
    pub(in crate::guest_init) protocol_version: u16,
}

pub(in crate::guest_init) fn run(
    command: &[String],
    identity: &DevIdentity,
    drop_to_identity: bool,
    config: ManagedSessionConfig,
) -> Result<()> {
    if config.protocol_version != PROTOCOL_VERSION {
        bail!(
            "loftd attach protocol version mismatch: guest supports {PROTOCOL_VERSION}, host requested {}",
            config.protocol_version
        );
    }
    let listener = VsockListener::bind(config.port)?;
    run_pre_start_event_loop(command, identity, drop_to_identity, listener)
}

fn run_pre_start_event_loop(
    command: &[String],
    identity: &DevIdentity,
    drop_to_identity: bool,
    listener: VsockListener,
) -> Result<()> {
    loop {
        let client = listener.accept()?;
        match handle_pre_start_client(client)? {
            PreStartClientResult::Wait => continue,
            PreStartClientResult::Start {
                mut client,
                initial_size,
            } => {
                let pty = match Pty::open() {
                    Ok(pty) => pty,
                    Err(err) => {
                        let _ = write_frame(&mut client, &Frame::Error(format!("{err:#}")));
                        return Err(err);
                    }
                };
                if let Some(size) = initial_size
                    && let Err(err) = set_winsize(pty.master.as_raw_fd(), size.rows, size.cols)
                {
                    let _ = write_frame(&mut client, &Frame::Error(format!("{err:#}")));
                    return Err(err);
                }
                let child = match spawn_pty_child(&pty, command, identity, drop_to_identity) {
                    Ok(child) => child,
                    Err(err) => {
                        let _ = write_frame(&mut client, &Frame::Error(format!("{err:#}")));
                        return Err(err);
                    }
                };
                let mut terminal_state = TerminalState::new(initial_size.unwrap_or_default());
                match serve_attached_client(
                    &pty.master,
                    child,
                    client,
                    &listener,
                    &mut terminal_state,
                )? {
                    ClientResult::Detached => {
                        return run_event_loop(pty.master, child, listener, terminal_state);
                    }
                    ClientResult::ChildExited(code) => std::process::exit(code),
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PtySize {
    rows: u16,
    cols: u16,
}

impl Default for PtySize {
    fn default() -> Self {
        Self {
            rows: DEFAULT_TERMINAL_ROWS,
            cols: DEFAULT_TERMINAL_COLS,
        }
    }
}

struct TerminalState {
    parser: vt100::Parser,
    output_normalizer: TerminalOutputNormalizer,
}

impl TerminalState {
    fn new(size: PtySize) -> Self {
        Self {
            parser: vt100::Parser::new(size.rows, size.cols, TERMINAL_SCROLLBACK_ROWS),
            output_normalizer: TerminalOutputNormalizer::default(),
        }
    }

    fn record_output(&mut self, bytes: &[u8]) -> Vec<u8> {
        let normalized = self.output_normalizer.normalize(bytes);
        self.parser.process(&normalized);
        normalized
    }

    fn resize(&mut self, size: PtySize) {
        self.parser.screen_mut().set_size(size.rows, size.cols);
    }

    #[cfg(test)]
    fn size(&self) -> PtySize {
        let (rows, cols) = self.parser.screen().size();
        PtySize { rows, cols }
    }

    fn render_restore(&self) -> Vec<u8> {
        let screen = self.parser.screen();
        let mut restore = Vec::new();
        if screen.alternate_screen() {
            restore.extend_from_slice(ENTER_ALTERNATE_SCREEN);
        } else {
            restore.extend_from_slice(EXIT_ALTERNATE_SCREEN);
        }
        restore.extend_from_slice(CLEAR_VISIBLE_SCREEN);
        restore.extend(screen.state_formatted());
        restore.extend(screen.cursor_state_formatted());
        restore.extend(screen.attributes_formatted());
        restore
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CharacterSet {
    Ascii,
    DecSpecialGraphics,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GraphicSet {
    G0,
    G1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NormalizerState {
    Ground,
    Escape,
    DesignateG0,
    DesignateG1,
    EscapePassthrough,
    CsiPassthrough,
    StringPassthrough,
    StringPassthroughEscaped,
}

#[derive(Debug)]
struct TerminalOutputNormalizer {
    g0: CharacterSet,
    g1: CharacterSet,
    active: GraphicSet,
    state: NormalizerState,
}

impl Default for TerminalOutputNormalizer {
    fn default() -> Self {
        Self {
            g0: CharacterSet::Ascii,
            g1: CharacterSet::Ascii,
            active: GraphicSet::G0,
            state: NormalizerState::Ground,
        }
    }
}

impl TerminalOutputNormalizer {
    fn normalize(&mut self, bytes: &[u8]) -> Vec<u8> {
        let mut output = Vec::with_capacity(bytes.len());
        for &byte in bytes {
            self.process_byte(byte, &mut output);
        }
        output
    }

    fn process_byte(&mut self, byte: u8, output: &mut Vec<u8>) {
        match self.state {
            NormalizerState::Ground => self.process_ground_byte(byte, output),
            NormalizerState::Escape => self.process_escape_byte(byte, output),
            NormalizerState::DesignateG0 => {
                self.process_designation_byte(byte, CharacterSetDesignator::G0, b'(', output)
            }
            NormalizerState::DesignateG1 => {
                self.process_designation_byte(byte, CharacterSetDesignator::G1, b')', output)
            }
            NormalizerState::EscapePassthrough => {
                output.push(byte);
                if is_escape_final_byte(byte) {
                    self.state = NormalizerState::Ground;
                }
            }
            NormalizerState::CsiPassthrough => {
                output.push(byte);
                if is_csi_final_byte(byte) {
                    self.state = NormalizerState::Ground;
                }
            }
            NormalizerState::StringPassthrough => {
                output.push(byte);
                match byte {
                    0x07 => self.state = NormalizerState::Ground,
                    0x1b => self.state = NormalizerState::StringPassthroughEscaped,
                    _ => {}
                }
            }
            NormalizerState::StringPassthroughEscaped => {
                output.push(byte);
                self.state = if byte == b'\\' {
                    NormalizerState::Ground
                } else {
                    NormalizerState::StringPassthrough
                };
            }
        }
    }

    fn process_ground_byte(&mut self, byte: u8, output: &mut Vec<u8>) {
        match byte {
            0x0e => self.active = GraphicSet::G1,
            0x0f => self.active = GraphicSet::G0,
            0x1b => self.state = NormalizerState::Escape,
            0x20..=0x7e if self.active_charset() == CharacterSet::DecSpecialGraphics => {
                if let Some(mapped) = dec_special_graphics_utf8(byte) {
                    output.extend_from_slice(mapped.as_bytes());
                } else {
                    output.push(byte);
                }
            }
            _ => output.push(byte),
        }
    }

    fn process_escape_byte(&mut self, byte: u8, output: &mut Vec<u8>) {
        match byte {
            b'(' => self.state = NormalizerState::DesignateG0,
            b')' => self.state = NormalizerState::DesignateG1,
            b'[' => {
                output.extend_from_slice(b"\x1b[");
                self.state = NormalizerState::CsiPassthrough;
            }
            b']' | b'P' | b'_' | b'^' | b'X' => {
                output.extend_from_slice(&[0x1b, byte]);
                self.state = NormalizerState::StringPassthrough;
            }
            b'c' => {
                self.reset();
                output.extend_from_slice(b"\x1bc");
                self.state = NormalizerState::Ground;
            }
            0x20..=0x2f => {
                output.extend_from_slice(&[0x1b, byte]);
                self.state = NormalizerState::EscapePassthrough;
            }
            _ => {
                output.extend_from_slice(&[0x1b, byte]);
                self.state = NormalizerState::Ground;
            }
        }
    }

    fn process_designation_byte(
        &mut self,
        byte: u8,
        designator: CharacterSetDesignator,
        intermediate: u8,
        output: &mut Vec<u8>,
    ) {
        match byte {
            b'0' => self.set_charset(designator, CharacterSet::DecSpecialGraphics),
            b'B' => self.set_charset(designator, CharacterSet::Ascii),
            _ => output.extend_from_slice(&[0x1b, intermediate, byte]),
        }
        self.state = NormalizerState::Ground;
    }

    fn active_charset(&self) -> CharacterSet {
        match self.active {
            GraphicSet::G0 => self.g0,
            GraphicSet::G1 => self.g1,
        }
    }

    fn set_charset(&mut self, designator: CharacterSetDesignator, charset: CharacterSet) {
        match designator {
            CharacterSetDesignator::G0 => self.g0 = charset,
            CharacterSetDesignator::G1 => self.g1 = charset,
        }
    }

    fn reset(&mut self) {
        self.g0 = CharacterSet::Ascii;
        self.g1 = CharacterSet::Ascii;
        self.active = GraphicSet::G0;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CharacterSetDesignator {
    G0,
    G1,
}

fn is_escape_final_byte(byte: u8) -> bool {
    (0x30..=0x7e).contains(&byte)
}

fn is_csi_final_byte(byte: u8) -> bool {
    (0x40..=0x7e).contains(&byte)
}

fn dec_special_graphics_utf8(byte: u8) -> Option<&'static str> {
    match byte {
        b'+' => Some("→"),
        b',' => Some("←"),
        b'-' => Some("↑"),
        b'.' => Some("↓"),
        b'0' => Some("▮"),
        b'`' => Some("◆"),
        b'a' => Some("▒"),
        b'b' => Some("␉"),
        b'c' => Some("␌"),
        b'd' => Some("␍"),
        b'e' => Some("␊"),
        b'f' => Some("°"),
        b'g' => Some("±"),
        b'h' => Some("␤"),
        b'i' => Some("␋"),
        b'j' => Some("┘"),
        b'k' => Some("┐"),
        b'l' => Some("┌"),
        b'm' => Some("└"),
        b'n' => Some("┼"),
        b'o' => Some("⎺"),
        b'p' => Some("⎻"),
        b'q' => Some("─"),
        b'r' => Some("⎼"),
        b's' => Some("⎽"),
        b't' => Some("├"),
        b'u' => Some("┤"),
        b'v' => Some("┴"),
        b'w' => Some("┬"),
        b'x' => Some("│"),
        b'y' => Some("≤"),
        b'z' => Some("≥"),
        b'{' => Some("π"),
        b'|' => Some("≠"),
        b'}' => Some("£"),
        b'~' => Some("·"),
        _ => None,
    }
}

enum PreStartClientResult<T> {
    Wait,
    Start {
        client: T,
        initial_size: Option<PtySize>,
    },
}

fn handle_pre_start_client<T>(mut client: T) -> Result<PreStartClientResult<T>>
where
    T: Read + Write,
{
    write_frame(
        &mut client,
        &Frame::Hello {
            version: PROTOCOL_VERSION,
        },
    )?;
    let mut initial_size = None;
    loop {
        match read_frame(&mut client)? {
            Some(Frame::Attach) => {
                return Ok(PreStartClientResult::Start {
                    client,
                    initial_size,
                });
            }
            Some(Frame::Resize { rows, cols }) => {
                initial_size = Some(PtySize { rows, cols });
            }
            Some(Frame::Detach) | None => return Ok(PreStartClientResult::Wait),
            Some(frame) => {
                write_frame(
                    &mut client,
                    &Frame::Error(format!("expected attach frame, got {frame:?}")),
                )?;
                return Ok(PreStartClientResult::Wait);
            }
        }
    }
}

fn run_event_loop(
    master: File,
    child: libc::pid_t,
    listener: VsockListener,
    mut terminal_state: TerminalState,
) -> Result<()> {
    loop {
        if let Some(code) = reap_child(child)? {
            std::process::exit(code);
        }
        let mut fds = [
            libc::pollfd {
                fd: listener.fd,
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: master.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        let rc =
            unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, POLL_TIMEOUT_MS) };
        if rc < 0 {
            let err = std::io::Error::last_os_error();
            if err.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Err(err).context("managed session poll failed");
        }
        if fds[1].revents & libc::POLLIN != 0 {
            drain_detached_pty_output(master.as_raw_fd(), &mut terminal_state)?;
        }
        if fds[0].revents & libc::POLLIN != 0 {
            let client = listener.accept()?;
            match serve_client(&master, child, client, &listener, &mut terminal_state)? {
                ClientResult::Detached => continue,
                ClientResult::ChildExited(code) => std::process::exit(code),
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClientResult {
    Detached,
    ChildExited(i32),
}

fn serve_client(
    master: &File,
    child: libc::pid_t,
    mut client: File,
    listener: &VsockListener,
    terminal_state: &mut TerminalState,
) -> Result<ClientResult> {
    write_frame(
        &mut client,
        &Frame::Hello {
            version: PROTOCOL_VERSION,
        },
    )?;
    if !read_post_start_attach_request(master, &mut client, terminal_state)? {
        return Ok(ClientResult::Detached);
    }
    drain_detached_pty_output(master.as_raw_fd(), terminal_state)?;
    let restore = terminal_state.render_restore();
    if !restore.is_empty() {
        write_frame(&mut client, &Frame::Data(restore))?;
    }
    serve_attached_client(master, child, client, listener, terminal_state)
}

fn read_post_start_attach_request<T>(
    master: &File,
    client: &mut T,
    terminal_state: &mut TerminalState,
) -> Result<bool>
where
    T: Read + Write,
{
    loop {
        match read_frame(client)? {
            Some(Frame::Attach) => return Ok(true),
            Some(Frame::Resize { rows, cols }) => {
                let size = PtySize { rows, cols };
                set_winsize(master.as_raw_fd(), rows, cols)?;
                terminal_state.resize(size);
            }
            Some(Frame::Detach) | None => return Ok(false),
            Some(frame) => {
                write_frame(
                    client,
                    &Frame::Error(format!("expected attach frame, got {frame:?}")),
                )?;
                return Ok(false);
            }
        }
    }
}

fn serve_attached_client(
    master: &File,
    child: libc::pid_t,
    mut client: File,
    listener: &VsockListener,
    terminal_state: &mut TerminalState,
) -> Result<ClientResult> {
    let active = Arc::new(AtomicBool::new(true));
    let reader_active = active.clone();
    let (resize_tx, resize_rx) = mpsc::channel();
    let mut client_reader = client.try_clone()?;
    let mut pty_writer = duplicate_file(master)?;
    let input_thread = thread::spawn(move || -> Result<()> {
        while reader_active.load(Ordering::SeqCst) {
            match read_frame(&mut client_reader)? {
                Some(Frame::Data(data)) => pty_writer.write_all(&data)?,
                Some(Frame::Resize { rows, cols }) => {
                    set_winsize(pty_writer.as_raw_fd(), rows, cols)?;
                    let _ = resize_tx.send(PtySize { rows, cols });
                }
                Some(Frame::Detach) | None => break,
                Some(Frame::Attach) => {}
                Some(frame) => bail!("unexpected attach client frame: {frame:?}"),
            }
        }
        reader_active.store(false, Ordering::SeqCst);
        Ok(())
    });

    let mut pty_reader = duplicate_file(master)?;
    let mut buf = [0u8; IO_BUF_SIZE];
    while active.load(Ordering::SeqCst) {
        apply_pending_resizes(&resize_rx, terminal_state);
        if let Some(code) = reap_child(child)? {
            let _ = write_frame(&mut client, &Frame::Exit { code });
            stop_client_input(&active, client.as_raw_fd(), input_thread);
            return Ok(ClientResult::ChildExited(code));
        }
        let mut fds = [
            libc::pollfd {
                fd: pty_reader.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: listener.fd,
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        let rc =
            unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, POLL_TIMEOUT_MS) };
        if rc < 0 {
            let err = std::io::Error::last_os_error();
            if err.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Err(err).context("managed session client poll failed");
        }
        if fds[1].revents & libc::POLLIN != 0
            && let Ok(mut extra_client) = listener.accept()
        {
            let _ = write_frame(&mut extra_client, &Frame::Busy);
        }
        if fds[0].revents & libc::POLLIN == 0 {
            continue;
        }
        let n = pty_reader.read(&mut buf)?;
        if n == 0 {
            thread::sleep(Duration::from_millis(10));
            continue;
        }
        let normalized = terminal_state.record_output(&buf[..n]);
        if write_frame(&mut client, &Frame::Data(normalized)).is_err() {
            break;
        }
    }
    apply_pending_resizes(&resize_rx, terminal_state);
    stop_client_input(&active, client.as_raw_fd(), input_thread);
    Ok(ClientResult::Detached)
}

fn apply_pending_resizes(resize_rx: &mpsc::Receiver<PtySize>, terminal_state: &mut TerminalState) {
    while let Ok(size) = resize_rx.try_recv() {
        terminal_state.resize(size);
    }
}

fn stop_client_input(
    active: &AtomicBool,
    client_fd: RawFd,
    input_thread: thread::JoinHandle<Result<()>>,
) {
    active.store(false, Ordering::SeqCst);
    let _ = unsafe { libc::shutdown(client_fd, libc::SHUT_RDWR) };
    let _ = input_thread.join();
}

fn drain_detached_pty_output(master_fd: RawFd, terminal_state: &mut TerminalState) -> Result<()> {
    let mut file = duplicate_fd(master_fd)?;
    set_nonblocking(file.as_raw_fd(), true)?;
    let result = drain_nonblocking(&mut file, terminal_state);
    let restore_result = set_nonblocking(file.as_raw_fd(), false);
    result.and(restore_result)
}

fn drain_nonblocking(file: &mut File, terminal_state: &mut TerminalState) -> Result<()> {
    let mut buf = [0u8; IO_BUF_SIZE];
    loop {
        match file.read(&mut buf) {
            Ok(0) => return Ok(()),
            Ok(n) => {
                terminal_state.record_output(&buf[..n]);
            }
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => return Ok(()),
            Err(err) => return Err(err).context("failed to drain detached PTY output"),
        }
    }
}

fn reap_child(child: libc::pid_t) -> Result<Option<i32>> {
    let mut status = 0;
    let rc = unsafe { libc::waitpid(child, &mut status, libc::WNOHANG) };
    if rc == 0 {
        return Ok(None);
    }
    if rc < 0 {
        let err = std::io::Error::last_os_error();
        if err.raw_os_error() == Some(libc::ECHILD) {
            return Ok(Some(0));
        }
        return Err(err).context("failed to wait for managed session child");
    }
    if libc::WIFEXITED(status) {
        Ok(Some(libc::WEXITSTATUS(status)))
    } else {
        Ok(Some(1))
    }
}

struct Pty {
    master: File,
    slave_path: String,
}

impl Pty {
    fn open() -> Result<Self> {
        let fd = unsafe { libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY) };
        if fd < 0 {
            return Err(std::io::Error::last_os_error()).context("failed to open PTY master");
        }
        if unsafe { libc::grantpt(fd) } != 0 {
            return Err(std::io::Error::last_os_error()).context("failed to grant PTY slave");
        }
        if unsafe { libc::unlockpt(fd) } != 0 {
            return Err(std::io::Error::last_os_error()).context("failed to unlock PTY slave");
        }
        let mut buf = vec![0i8; 256];
        if unsafe { libc::ptsname_r(fd, buf.as_mut_ptr(), buf.len()) } != 0 {
            return Err(std::io::Error::last_os_error())
                .context("failed to resolve PTY slave path");
        }
        let cstr = unsafe { std::ffi::CStr::from_ptr(buf.as_ptr()) };
        Ok(Self {
            master: unsafe { File::from_raw_fd(fd) },
            slave_path: cstr.to_string_lossy().into_owned(),
        })
    }
}

fn spawn_pty_child(
    pty: &Pty,
    command: &[String],
    identity: &DevIdentity,
    drop_to_identity: bool,
) -> Result<libc::pid_t> {
    let pid = unsafe { libc::fork() };
    if pid < 0 {
        bail!(
            "failed to fork managed session child: {}",
            std::io::Error::last_os_error()
        );
    }
    if pid == 0 {
        let code = child_main(&pty.slave_path, command, identity, drop_to_identity);
        std::process::exit(code);
    }
    Ok(pid)
}

fn child_main(
    slave_path: &str,
    command: &[String],
    identity: &DevIdentity,
    drop_to_identity: bool,
) -> i32 {
    if let Err(err) = child_main_result(slave_path, command, identity, drop_to_identity) {
        eprintln!("loftd-guest-init managed session child: {err:#}");
        127
    } else {
        0
    }
}

fn child_main_result(
    slave_path: &str,
    command: &[String],
    identity: &DevIdentity,
    drop_to_identity: bool,
) -> Result<()> {
    if unsafe { libc::setsid() } < 0 {
        return Err(std::io::Error::last_os_error()).context("failed to create managed session");
    }
    let slave = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(slave_path)
        .with_context(|| format!("failed to open PTY slave {slave_path}"))?;
    if unsafe { libc::ioctl(slave.as_raw_fd(), libc::TIOCSCTTY, 0) } != 0 {
        return Err(std::io::Error::last_os_error()).context("failed to set controlling PTY");
    }
    for fd in [libc::STDIN_FILENO, libc::STDOUT_FILENO, libc::STDERR_FILENO] {
        if unsafe { libc::dup2(slave.as_raw_fd(), fd) } < 0 {
            return Err(std::io::Error::last_os_error()).context("failed to wire PTY stdio");
        }
    }
    if drop_to_identity {
        process::drop_to_identity_and_exec(identity, command)
    } else {
        process::exec_command(command)
    }
}

struct VsockListener {
    fd: RawFd,
}

impl VsockListener {
    fn bind(port: u32) -> Result<Self> {
        let fd = unsafe { libc::socket(libc::AF_VSOCK, libc::SOCK_STREAM, 0) };
        if fd < 0 {
            return Err(std::io::Error::last_os_error())
                .context("failed to create AF_VSOCK socket");
        }
        let listener = Self { fd };
        let addr = libc::sockaddr_vm {
            svm_family: libc::AF_VSOCK as libc::sa_family_t,
            svm_reserved1: 0,
            svm_port: port,
            svm_cid: libc::VMADDR_CID_ANY,
            svm_zero: [0; 4],
        };
        let rc = unsafe {
            libc::bind(
                listener.fd,
                (&addr as *const libc::sockaddr_vm).cast::<libc::sockaddr>(),
                std::mem::size_of::<libc::sockaddr_vm>() as libc::socklen_t,
            )
        };
        if rc != 0 {
            return Err(std::io::Error::last_os_error())
                .context("failed to bind AF_VSOCK listener");
        }
        if unsafe { libc::listen(listener.fd, 1) } != 0 {
            return Err(std::io::Error::last_os_error())
                .context("failed to listen on AF_VSOCK socket");
        }
        Ok(listener)
    }

    fn accept(&self) -> Result<File> {
        let fd = unsafe { libc::accept(self.fd, std::ptr::null_mut(), std::ptr::null_mut()) };
        if fd < 0 {
            return Err(std::io::Error::last_os_error()).context("failed to accept attach client");
        }
        Ok(unsafe { File::from_raw_fd(fd) })
    }
}

impl Drop for VsockListener {
    fn drop(&mut self) {
        let _ = unsafe { libc::close(self.fd) };
    }
}

fn set_winsize(fd: RawFd, rows: u16, cols: u16) -> Result<()> {
    let size = libc::winsize {
        ws_row: rows,
        ws_col: cols,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    if unsafe { libc::ioctl(fd, libc::TIOCSWINSZ, &size) } != 0 {
        return Err(std::io::Error::last_os_error()).context("failed to resize managed PTY");
    }
    Ok(())
}

fn duplicate_file(file: &File) -> Result<File> {
    duplicate_fd(file.as_raw_fd())
}

fn duplicate_fd(fd: RawFd) -> Result<File> {
    let dup = unsafe { libc::dup(fd) };
    if dup < 0 {
        return Err(std::io::Error::last_os_error()).context("failed to duplicate fd");
    }
    Ok(unsafe { File::from_raw_fd(dup) })
}

fn set_nonblocking(fd: RawFd, enabled: bool) -> Result<()> {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 {
        return Err(std::io::Error::last_os_error()).context("failed to read fd flags");
    }
    let next = if enabled {
        flags | libc::O_NONBLOCK
    } else {
        flags & !libc::O_NONBLOCK
    };
    if unsafe { libc::fcntl(fd, libc::F_SETFL, next) } < 0 {
        return Err(std::io::Error::last_os_error()).context("failed to update fd flags");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::fd::IntoRawFd;
    use std::os::unix::net::{UnixListener, UnixStream};

    #[test]
    fn managed_session_rejects_protocol_mismatch() {
        assert_ne!(
            ManagedSessionConfig {
                port: 1,
                protocol_version: PROTOCOL_VERSION + 1,
            },
            ManagedSessionConfig {
                port: 1,
                protocol_version: PROTOCOL_VERSION,
            }
        );
    }

    #[test]
    fn pre_start_detach_is_readiness_only() {
        let (mut client, server) = UnixStream::pair().unwrap();
        let server = thread::spawn(move || handle_pre_start_client(server));

        assert_eq!(
            read_frame(&mut client).unwrap(),
            Some(Frame::Hello {
                version: PROTOCOL_VERSION
            })
        );
        write_frame(&mut client, &Frame::Detach).unwrap();

        match server.join().unwrap().unwrap() {
            PreStartClientResult::Wait => {}
            PreStartClientResult::Start { .. } => panic!("readiness detach started command"),
        }
    }

    #[test]
    fn pre_start_resize_does_not_start_command() {
        let (mut client, server) = UnixStream::pair().unwrap();
        let server = thread::spawn(move || handle_pre_start_client(server));

        assert_eq!(
            read_frame(&mut client).unwrap(),
            Some(Frame::Hello {
                version: PROTOCOL_VERSION
            })
        );
        write_frame(
            &mut client,
            &Frame::Resize {
                rows: 40,
                cols: 120,
            },
        )
        .unwrap();
        write_frame(&mut client, &Frame::Detach).unwrap();

        match server.join().unwrap().unwrap() {
            PreStartClientResult::Wait => {}
            PreStartClientResult::Start { .. } => panic!("pre-attach resize started command"),
        }
    }

    #[test]
    fn pre_start_attach_returns_start_with_latest_resize() {
        let (mut client, server) = UnixStream::pair().unwrap();
        let server = thread::spawn(move || handle_pre_start_client(server));

        assert_eq!(
            read_frame(&mut client).unwrap(),
            Some(Frame::Hello {
                version: PROTOCOL_VERSION
            })
        );
        write_frame(&mut client, &Frame::Resize { rows: 24, cols: 80 }).unwrap();
        write_frame(
            &mut client,
            &Frame::Resize {
                rows: 50,
                cols: 160,
            },
        )
        .unwrap();
        write_frame(&mut client, &Frame::Attach).unwrap();

        match server.join().unwrap().unwrap() {
            PreStartClientResult::Start { initial_size, .. } => {
                assert_eq!(
                    initial_size,
                    Some(PtySize {
                        rows: 50,
                        cols: 160
                    })
                );
            }
            PreStartClientResult::Wait => panic!("real attach did not start command"),
        }
    }

    #[test]
    fn pre_start_invalid_frame_errors_without_starting() {
        let (mut client, server) = UnixStream::pair().unwrap();
        let server = thread::spawn(move || handle_pre_start_client(server));

        assert_eq!(
            read_frame(&mut client).unwrap(),
            Some(Frame::Hello {
                version: PROTOCOL_VERSION
            })
        );
        write_frame(&mut client, &Frame::Data(b"lost input".to_vec())).unwrap();
        match read_frame(&mut client).unwrap() {
            Some(Frame::Error(message)) => assert!(message.contains("expected attach frame")),
            frame => panic!("expected error frame, got {frame:?}"),
        }

        match server.join().unwrap().unwrap() {
            PreStartClientResult::Wait => {}
            PreStartClientResult::Start { .. } => panic!("invalid pre-start data started command"),
        }
    }

    #[test]
    fn attached_client_receives_initial_pty_output() {
        let pty = Pty::open().unwrap();
        let child = unsafe { libc::fork() };
        assert!(child >= 0);
        if child == 0 {
            let result = write_then_sleep_on_pty_slave(&pty.slave_path, b"primary-da-visible\n");
            std::process::exit(if result.is_ok() { 0 } else { 1 });
        }

        let temp = tempfile::tempdir().unwrap();
        let unix_listener = UnixListener::bind(temp.path().join("listener.sock")).unwrap();
        let listener = VsockListener {
            fd: unix_listener.into_raw_fd(),
        };
        let (mut client, server) = UnixStream::pair().unwrap();
        let server_file = unsafe { File::from_raw_fd(server.into_raw_fd()) };

        let server = thread::spawn(move || {
            let mut terminal_state = TerminalState::new(PtySize::default());
            serve_attached_client(
                &pty.master,
                child,
                server_file,
                &listener,
                &mut terminal_state,
            )
        });

        client
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        assert_data_frames_eq(&mut client, b"primary-da-visible\r\n");
        write_frame(&mut client, &Frame::Detach).unwrap();
        assert_eq!(server.join().unwrap().unwrap(), ClientResult::Detached);
        let mut status = 0;
        assert_eq!(unsafe { libc::waitpid(child, &mut status, 0) }, child);
        assert!(libc::WIFEXITED(status));
        assert_eq!(libc::WEXITSTATUS(status), 0);
    }

    #[test]
    fn terminal_state_returns_normalized_output_for_live_proxy() {
        let mut terminal_state = TerminalState::new(PtySize::default());

        let live_data = terminal_state.record_output(b"\x1b(0qx\x1b(B\n");

        assert_eq!(live_data, "─│\n".as_bytes());
        assert!(terminal_state.parser.screen().contents().contains("─│"));
    }

    #[test]
    fn terminal_restore_repaints_visible_screen_and_cursor_state() {
        let mut terminal_state = TerminalState::new(PtySize { rows: 10, cols: 40 });
        terminal_state.record_output(b"prompt> abc");
        terminal_state.record_output(b"\x1b[3DXYZ");

        let restored = restored_screen(&terminal_state);

        assert!(restored.screen().contents().contains("prompt> XYZ"));
        assert_eq!(
            restored.screen().cursor_position(),
            terminal_state.parser.screen().cursor_position()
        );
    }

    #[test]
    fn terminal_restore_clears_stale_cells() {
        let mut terminal_state = TerminalState::new(PtySize { rows: 5, cols: 40 });
        terminal_state.record_output(b"short");

        let mut restored = vt100::Parser::new(5, 40, 0);
        restored.process(b"this line used to be much longer\r\nstale lower row");
        restored.process(&terminal_state.render_restore());

        let first_row = restored.screen().rows(0, 40).next().unwrap();
        assert_eq!(first_row.trim_end(), "short");
        assert!(!first_row.contains("longer"));
        assert!(!restored.screen().contents().contains("stale lower row"));
    }

    #[test]
    fn terminal_restore_renders_acs_borders_as_unicode() {
        let mut terminal_state = TerminalState::new(PtySize { rows: 5, cols: 20 });
        terminal_state.record_output(b"\x1b(0lqqk\r\nmqqj\x1b(B");

        let restored = restored_screen(&terminal_state);
        let contents = restored.screen().contents();

        assert!(contents.contains("┌──┐"), "{contents:?}");
        assert!(contents.contains("└──┘"), "{contents:?}");
        assert!(!contents.contains("lqqk"), "{contents:?}");
        assert!(!contents.contains("mqqj"), "{contents:?}");
    }

    #[test]
    fn terminal_restore_preserves_alternate_screen_and_input_modes() {
        let mut terminal_state = TerminalState::new(PtySize { rows: 10, cols: 40 });
        terminal_state.record_output(b"\x1b[?1049h\x1b[?25l\x1b[?1h\x1b[?2004hinside-tui");

        let restored = restored_screen(&terminal_state);
        let screen = restored.screen();

        assert!(screen.alternate_screen());
        assert!(screen.hide_cursor());
        assert!(screen.application_cursor());
        assert!(screen.bracketed_paste());
        assert!(screen.contents().contains("inside-tui"));
    }

    #[test]
    fn post_start_attach_resize_updates_restore_dimensions() {
        let pty = Pty::open().unwrap();
        let (mut client, server) = UnixStream::pair().unwrap();
        let mut server_file = unsafe { File::from_raw_fd(server.into_raw_fd()) };

        let server = thread::spawn(move || {
            let mut terminal_state = TerminalState::new(PtySize::default());
            let attached =
                read_post_start_attach_request(&pty.master, &mut server_file, &mut terminal_state)
                    .unwrap();
            (attached, terminal_state.size())
        });

        write_frame(
            &mut client,
            &Frame::Resize {
                rows: 42,
                cols: 132,
            },
        )
        .unwrap();
        write_frame(&mut client, &Frame::Attach).unwrap();

        let (attached, size) = server.join().unwrap();
        assert!(attached);
        assert_eq!(
            size,
            PtySize {
                rows: 42,
                cols: 132
            }
        );
    }

    #[test]
    fn post_start_attach_sends_detached_restore_before_live_proxy() {
        let pty = Pty::open().unwrap();
        let child = unsafe { libc::fork() };
        assert!(child >= 0);
        if child == 0 {
            let result = write_then_sleep_on_pty_slave(&pty.slave_path, b"detached-visible\n");
            std::process::exit(if result.is_ok() { 0 } else { 1 });
        }
        thread::sleep(Duration::from_millis(50));

        let mut terminal_state = TerminalState::new(PtySize::default());
        drain_detached_pty_output(pty.master.as_raw_fd(), &mut terminal_state).unwrap();

        let temp = tempfile::tempdir().unwrap();
        let unix_listener = UnixListener::bind(temp.path().join("listener.sock")).unwrap();
        let listener = VsockListener {
            fd: unix_listener.into_raw_fd(),
        };
        let (mut client, server) = UnixStream::pair().unwrap();
        let server_file = unsafe { File::from_raw_fd(server.into_raw_fd()) };

        let server = thread::spawn(move || {
            serve_client(
                &pty.master,
                child,
                server_file,
                &listener,
                &mut terminal_state,
            )
        });

        assert_eq!(
            read_frame(&mut client).unwrap(),
            Some(Frame::Hello {
                version: PROTOCOL_VERSION
            })
        );
        write_frame(&mut client, &Frame::Attach).unwrap();

        match read_frame(&mut client).unwrap() {
            Some(Frame::Data(data)) => {
                let mut restored = vt100::Parser::new(24, 80, 0);
                restored.process(&data);
                assert!(restored.screen().contents().contains("detached-visible"));
            }
            frame => panic!("expected restore frame, got {frame:?}"),
        }

        write_frame(&mut client, &Frame::Detach).unwrap();
        assert_eq!(server.join().unwrap().unwrap(), ClientResult::Detached);
        let mut status = 0;
        assert_eq!(unsafe { libc::waitpid(child, &mut status, 0) }, child);
    }

    #[test]
    fn normalizer_maps_full_tmux_base_acs_table() {
        let mut normalizer = TerminalOutputNormalizer::default();
        let input = b"\x1b(0+,-.0`abcdefghijklmnopqrstuvwxyz{|}~";

        let normalized = normalizer.normalize(input);

        assert_eq!(
            String::from_utf8(normalized).unwrap(),
            "→←↑↓▮◆▒␉␌␍␊°±␤␋┘┐┌└┼⎺⎻─⎼⎽├┤┴┬│≤≥π≠£·"
        );
    }

    #[test]
    fn normalizer_preserves_pending_designation_across_reads() {
        let mut normalizer = TerminalOutputNormalizer::default();

        assert!(normalizer.normalize(b"\x1b(").is_empty());
        assert_eq!(normalizer.normalize(b"0q"), "─".as_bytes());
        assert!(normalizer.normalize(b"\x1b)").is_empty());
        assert!(normalizer.normalize(b"0").is_empty());
        assert_eq!(normalizer.normalize(b"\x0ex"), "│".as_bytes());
    }

    #[test]
    fn normalizer_tracks_g1_shift_in_and_shift_out() {
        let mut normalizer = TerminalOutputNormalizer::default();

        let normalized = normalizer.normalize(b"\x1b)0\x0ex\x0fx");

        assert_eq!(normalized, "│x".as_bytes());
    }

    #[test]
    fn normalizer_does_not_reset_charset_on_sgr() {
        let mut normalizer = TerminalOutputNormalizer::default();

        let normalized = normalizer.normalize(b"\x1b(0q\x1b[0mq\x1b(Bq");

        assert_eq!(normalized, "\u{2500}\x1b[0m\u{2500}q".as_bytes());
    }

    #[test]
    fn normalizer_passes_unrelated_escape_sequences_through() {
        let mut normalizer = TerminalOutputNormalizer::default();

        let normalized = normalizer.normalize(b"\x1b(0\x1b%Gq\x1b]0;x\x07q");

        assert_eq!(normalized, "\x1b%G─\x1b]0;x\x07─".as_bytes());
    }

    #[test]
    fn normalizer_passes_utf8_bytes_through() {
        let mut normalizer = TerminalOutputNormalizer::default();

        let normalized = normalizer.normalize("é\x1b(0qé".as_bytes());

        assert_eq!(normalized, "é─é".as_bytes());
    }

    fn assert_data_frames_eq<T>(client: &mut T, expected: &[u8])
    where
        T: Read,
    {
        let mut received = Vec::new();
        while received.len() < expected.len() {
            match read_frame(client).unwrap() {
                Some(Frame::Data(data)) => received.extend(data),
                frame => panic!("expected PTY data frame, got {frame:?}"),
            }
        }
        assert_eq!(received, expected);
    }

    fn restored_screen(terminal_state: &TerminalState) -> vt100::Parser {
        let size = terminal_state.size();
        let mut restored = vt100::Parser::new(size.rows, size.cols, 0);
        restored.process(&terminal_state.render_restore());
        restored
    }

    fn write_then_sleep_on_pty_slave(slave_path: &str, data: &[u8]) -> Result<()> {
        let mut slave = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(slave_path)?;
        slave.write_all(data)?;
        slave.flush()?;
        thread::sleep(Duration::from_millis(200));
        Ok(())
    }
}
