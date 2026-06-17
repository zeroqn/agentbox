use anyhow::{Context, Result, bail};
use loftd_attach_protocol::{Frame, PROTOCOL_VERSION, read_frame, write_frame};
use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, RawFd};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread;
use std::time::Duration;

use crate::guest_init::components::home::identity::DevIdentity;
use crate::guest_init::process;

const IO_BUF_SIZE: usize = 16 * 1024;
const POLL_TIMEOUT_MS: i32 = 100;

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
    let pty = Pty::open()?;
    let child = spawn_pty_child(&pty, command, identity, drop_to_identity)?;
    let listener = VsockListener::bind(config.port)?;
    run_event_loop(pty.master, child, listener)
}

fn run_event_loop(master: File, child: libc::pid_t, listener: VsockListener) -> Result<()> {
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
            drain_detached_pty_output(master.as_raw_fd())?;
        }
        if fds[0].revents & libc::POLLIN != 0 {
            let client = listener.accept()?;
            match serve_client(&master, child, client, &listener)? {
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
) -> Result<ClientResult> {
    write_frame(
        &mut client,
        &Frame::Hello {
            version: PROTOCOL_VERSION,
        },
    )?;
    match read_frame(&mut client)? {
        Some(Frame::Attach) => {}
        Some(Frame::Detach) | None => return Ok(ClientResult::Detached),
        Some(frame) => {
            write_frame(
                &mut client,
                &Frame::Error(format!("expected attach frame, got {frame:?}")),
            )?;
            return Ok(ClientResult::Detached);
        }
    }

    let active = Arc::new(AtomicBool::new(true));
    let reader_active = active.clone();
    let mut client_reader = client.try_clone()?;
    let mut pty_writer = duplicate_file(master)?;
    let input_thread = thread::spawn(move || -> Result<()> {
        while reader_active.load(Ordering::SeqCst) {
            match read_frame(&mut client_reader)? {
                Some(Frame::Data(data)) => pty_writer.write_all(&data)?,
                Some(Frame::Resize { rows, cols }) => {
                    set_winsize(pty_writer.as_raw_fd(), rows, cols)?
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
        if write_frame(&mut client, &Frame::Data(buf[..n].to_vec())).is_err() {
            break;
        }
    }
    stop_client_input(&active, client.as_raw_fd(), input_thread);
    Ok(ClientResult::Detached)
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

fn drain_detached_pty_output(master_fd: RawFd) -> Result<()> {
    let mut file = duplicate_fd(master_fd)?;
    set_nonblocking(file.as_raw_fd(), true)?;
    let result = drain_nonblocking(&mut file);
    let restore_result = set_nonblocking(file.as_raw_fd(), false);
    result.and(restore_result)
}

fn drain_nonblocking(file: &mut File) -> Result<()> {
    let mut buf = [0u8; IO_BUF_SIZE];
    loop {
        match file.read(&mut buf) {
            Ok(0) => return Ok(()),
            Ok(_) => continue,
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
}
