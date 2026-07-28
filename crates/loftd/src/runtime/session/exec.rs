use anyhow::{Context, Result, anyhow, bail};
use loftd_exec_protocol::{Frame, PROTOCOL_VERSION, read_frame, write_frame};
use std::io::{Read, Write};
use std::mem::MaybeUninit;
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::process::ExitCode;

use crate::runtime::session::task_control::{
    ActiveTaskStatus, ProcessInspector, ProcfsInspector, list_records, resolve_task_selector,
};

const IO_BUF_SIZE: usize = 16 * 1024;
const FORWARDED_SIGNALS: [i32; 3] = [libc::SIGINT, libc::SIGTERM, libc::SIGHUP];

pub(crate) fn exec_in_task(
    app_dir: &Path,
    task_selector: &str,
    argv: Vec<String>,
    inspector: &impl ProcessInspector,
) -> Result<ExitCode> {
    if argv.is_empty() {
        bail!("loftd exec requires a command");
    }
    let records = list_records(app_dir)?;
    let record = resolve_task_selector(&records, task_selector)?;
    match inspector.status(&record.process) {
        ActiveTaskStatus::Running => {}
        status => bail!(
            "loftd task '{}' cannot execute commands because it is {}",
            record.task_id,
            status.as_str()
        ),
    }
    let exec = record.exec.as_ref().ok_or_else(|| {
        anyhow!(
            "loftd task '{}' does not support exec; relaunch it with a current loftd version",
            record.task_id
        )
    })?;
    if exec.protocol_version != PROTOCOL_VERSION {
        bail!(
            "loftd exec protocol version mismatch: host supports {PROTOCOL_VERSION}, task uses {}",
            exec.protocol_version
        );
    }
    exec_on_socket(&exec.socket, argv)
}

pub(crate) fn exec_in_task_with_procfs(
    app_dir: &Path,
    task_selector: &str,
    argv: Vec<String>,
) -> Result<ExitCode> {
    exec_in_task(app_dir, task_selector, argv, &ProcfsInspector)
}

fn exec_on_socket(socket: &Path, argv: Vec<String>) -> Result<ExitCode> {
    let mut stream = UnixStream::connect(socket).with_context(|| {
        format!(
            "failed to connect to loftd exec socket '{}'",
            socket.display()
        )
    })?;
    write_frame(
        &mut stream,
        &Frame::Hello {
            version: PROTOCOL_VERSION,
        },
    )?;
    match read_frame(&mut stream)? {
        Some(Frame::Ready) => {}
        Some(Frame::Error(message)) => bail!("{message}"),
        Some(frame) => bail!("unexpected loftd exec handshake frame: {frame:?}"),
        None => bail!("loftd exec connection closed during handshake"),
    }
    write_frame(&mut stream, &Frame::Start { argv })?;

    let signal_mask = BlockedExecSignals::block()?;
    let signal_fd = SignalFd::new(signal_mask.set())?;
    let stdin = std::io::stdin();
    let mut stdin_open = true;
    let mut stdout = std::io::stdout().lock();
    let mut stderr = std::io::stderr().lock();
    let mut input = vec![0; IO_BUF_SIZE];

    loop {
        let mut fds = [
            libc::pollfd {
                fd: stream.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: if stdin_open { stdin.as_raw_fd() } else { -1 },
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: signal_fd.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        let rc = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, -1) };
        if rc < 0 {
            let err = std::io::Error::last_os_error();
            if err.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Err(err).context("loftd exec poll failed");
        }

        if fds[0].revents & (libc::POLLIN | libc::POLLHUP | libc::POLLERR) != 0 {
            match read_frame(&mut stream)? {
                Some(Frame::Stdout(data)) => {
                    stdout.write_all(&data)?;
                    stdout.flush()?;
                }
                Some(Frame::Stderr(data)) => {
                    stderr.write_all(&data)?;
                    stderr.flush()?;
                }
                Some(Frame::Exit { code }) => {
                    return Ok(ExitCode::from(u8::try_from(code).unwrap_or(1)));
                }
                Some(Frame::Error(message)) => return Err(anyhow!(message)),
                Some(frame) => return Err(anyhow!("unexpected loftd exec frame: {frame:?}")),
                None => return Err(anyhow!("loftd exec connection closed before exit status")),
            }
        }

        if stdin_open && fds[1].revents & (libc::POLLIN | libc::POLLHUP | libc::POLLERR) != 0 {
            match stdin.lock().read(&mut input) {
                Ok(0) => {
                    write_frame(&mut stream, &Frame::StdinEof)?;
                    stdin_open = false;
                }
                Ok(read) => write_frame(&mut stream, &Frame::Stdin(input[..read].to_vec()))?,
                Err(err) if err.kind() == std::io::ErrorKind::Interrupted => {}
                Err(err) => return Err(err).context("failed to read loftd exec stdin"),
            }
        }

        if fds[2].revents & libc::POLLIN != 0 {
            write_frame(
                &mut stream,
                &Frame::Signal {
                    signal: signal_fd.read_signal()?,
                },
            )?;
        }
    }
}

struct BlockedExecSignals {
    set: libc::sigset_t,
    previous: libc::sigset_t,
}

impl BlockedExecSignals {
    fn block() -> Result<Self> {
        let mut set = unsafe { std::mem::zeroed::<libc::sigset_t>() };
        if unsafe { libc::sigemptyset(&mut set) } != 0 {
            return Err(std::io::Error::last_os_error()).context("failed to initialize signal set");
        }
        for signal in FORWARDED_SIGNALS {
            if unsafe { libc::sigaddset(&mut set, signal) } != 0 {
                return Err(std::io::Error::last_os_error())
                    .context("failed to add loftd exec signal to signal set");
            }
        }
        let mut previous = MaybeUninit::<libc::sigset_t>::uninit();
        let rc = unsafe { libc::pthread_sigmask(libc::SIG_BLOCK, &set, previous.as_mut_ptr()) };
        if rc != 0 {
            return Err(std::io::Error::from_raw_os_error(rc))
                .context("failed to block loftd exec signals");
        }
        Ok(Self {
            set,
            previous: unsafe { previous.assume_init() },
        })
    }

    fn set(&self) -> &libc::sigset_t {
        &self.set
    }
}

impl Drop for BlockedExecSignals {
    fn drop(&mut self) {
        let _ = unsafe {
            libc::pthread_sigmask(libc::SIG_SETMASK, &self.previous, std::ptr::null_mut())
        };
    }
}

struct SignalFd(i32);

impl SignalFd {
    fn new(set: &libc::sigset_t) -> Result<Self> {
        let fd = unsafe { libc::signalfd(-1, set, libc::SFD_CLOEXEC | libc::SFD_NONBLOCK) };
        if fd < 0 {
            return Err(std::io::Error::last_os_error())
                .context("failed to create loftd exec signal fd");
        }
        Ok(Self(fd))
    }

    fn as_raw_fd(&self) -> i32 {
        self.0
    }

    fn read_signal(&self) -> Result<i32> {
        let mut info = MaybeUninit::<libc::signalfd_siginfo>::uninit();
        let read = unsafe {
            libc::read(
                self.0,
                info.as_mut_ptr().cast(),
                std::mem::size_of::<libc::signalfd_siginfo>(),
            )
        };
        if read != std::mem::size_of::<libc::signalfd_siginfo>() as isize {
            return Err(std::io::Error::last_os_error())
                .context("failed to read loftd exec signal fd");
        }
        Ok(unsafe { info.assume_init() }.ssi_signo as i32)
    }
}

impl Drop for SignalFd {
    fn drop(&mut self) {
        let _ = unsafe { libc::close(self.0) };
    }
}
