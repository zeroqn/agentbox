//! Helper-side managed attach socket readiness and permission repair.

use anyhow::{Context, Result, bail};
use loftd_attach_protocol::{Frame, PROTOCOL_VERSION, read_frame, write_frame};
use std::ffi::CString;
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use crate::runtime::launch::config::ManagedSessionConfig;
use crate::runtime::session::supervisor::identity::{
    FilesystemIdentityScope, RealFilesystemIdentityScope,
};
use crate::runtime::session::supervisor::vm_child::VmWorkerGuard;
use crate::runtime::vm::network::status_exit_code;

const READY_TIMEOUT: Duration = Duration::from_secs(30);
const READY_POLL: Duration = Duration::from_millis(25);
const HANDSHAKE_PROBE_READ_TIMEOUT: Duration = Duration::from_secs(1);
const ATTACH_SOCKET_MODE: u32 = 0o600;

pub(crate) fn wait_for_managed_attach_socket(
    managed: &ManagedSessionConfig,
    worker: &mut VmWorkerGuard,
) -> Result<()> {
    wait_for_managed_attach_socket_with(
        managed,
        ReadyOptions::default(),
        &RealSocketOps,
        &RealFilesystemIdentityScope,
        &RealHandshakeProbe,
        || worker.try_wait(),
    )
}

#[derive(Debug, Clone, Copy)]
struct ReadyOptions {
    timeout: Duration,
    poll: Duration,
}

impl Default for ReadyOptions {
    fn default() -> Self {
        Self {
            timeout: READY_TIMEOUT,
            poll: READY_POLL,
        }
    }
}

trait SocketOps {
    fn symlink_metadata(&self, path: &Path) -> std::io::Result<fs::Metadata>;
    fn chown(&self, path: &Path, uid: u32, gid: u32) -> std::io::Result<()>;
    fn chmod(&self, path: &Path, mode: u32) -> std::io::Result<()>;
}

struct RealSocketOps;

impl SocketOps for RealSocketOps {
    fn symlink_metadata(&self, path: &Path) -> std::io::Result<fs::Metadata> {
        fs::symlink_metadata(path)
    }

    fn chown(&self, path: &Path, uid: u32, gid: u32) -> std::io::Result<()> {
        let path = c_path(path)?;
        // SAFETY: path is NUL-terminated; fchownat is called with no-follow so
        // a replaced symlink is not followed during ownership repair.
        let rc = unsafe {
            libc::fchownat(
                libc::AT_FDCWD,
                path.as_ptr(),
                uid,
                gid,
                libc::AT_SYMLINK_NOFOLLOW,
            )
        };
        if rc == 0 {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error())
        }
    }

    fn chmod(&self, path: &Path, mode: u32) -> std::io::Result<()> {
        let path = c_path(path)?;
        // SAFETY: path is NUL-terminated; fchmodat is called with no-follow so
        // a replaced symlink is not followed during mode repair.
        let rc = unsafe {
            libc::fchmodat(
                libc::AT_FDCWD,
                path.as_ptr(),
                mode as libc::mode_t,
                libc::AT_SYMLINK_NOFOLLOW,
            )
        };
        if rc == 0 {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error())
        }
    }
}

fn wait_for_managed_attach_socket_with<F>(
    managed: &ManagedSessionConfig,
    options: ReadyOptions,
    ops: &impl SocketOps,
    identity_scope: &impl FilesystemIdentityScope,
    handshake_probe: &impl HandshakeProbe,
    mut poll_worker: F,
) -> Result<()>
where
    F: FnMut() -> Result<Option<i32>>,
{
    let deadline = Instant::now() + options.timeout;
    loop {
        if let Some(status) = poll_worker()? {
            let status = status_description(status);
            bail!("vm-worker-exited-before-ready: {status}");
        }
        match prepare_attach_socket(managed, ops, identity_scope) {
            Ok(()) => {
                match handshake_probe.probe(&managed.attach_socket, HANDSHAKE_PROBE_READ_TIMEOUT) {
                    Ok(HandshakeProbeOutcome::Ready) => return Ok(()),
                    Ok(HandshakeProbeOutcome::NotReady) => {}
                    Err(err) => return Err(err),
                }
            }
            Err(ReadyProbeError::Missing) => {}
            Err(ReadyProbeError::Fatal(err)) => return Err(err),
        }
        if Instant::now() >= deadline {
            bail!(
                "timed out waiting for libkrun attach socket handshake at '{}'",
                managed.attach_socket.display()
            );
        }
        thread::sleep(options.poll);
    }
}

enum ReadyProbeError {
    Missing,
    Fatal(anyhow::Error),
}

fn prepare_attach_socket(
    managed: &ManagedSessionConfig,
    ops: &impl SocketOps,
    identity_scope: &impl FilesystemIdentityScope,
) -> std::result::Result<(), ReadyProbeError> {
    let path = &managed.attach_socket;
    let metadata = match ops.symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Err(ReadyProbeError::Missing);
        }
        Err(err) => {
            let err = anyhow::Error::new(err).context(format!(
                "failed to stat libkrun attach socket '{}'",
                path.display()
            ));
            return Err(ReadyProbeError::Fatal(err));
        }
    };
    if !metadata.file_type().is_socket() {
        return Err(ReadyProbeError::Fatal(anyhow::anyhow!(
            "libkrun attach path '{}' exists but is not a Unix socket",
            path.display()
        )));
    }
    identity_scope
        .with_namespace_root(|| repair_attach_socket_with_namespace_root(managed, ops))
        .with_context(|| {
            format!(
                "failed to repair libkrun attach socket '{}' with namespace-root filesystem identity",
                path.display()
            )
        })
        .map_err(ReadyProbeError::Fatal)
}

fn repair_attach_socket_with_namespace_root(
    managed: &ManagedSessionConfig,
    ops: &impl SocketOps,
) -> Result<()> {
    let path = &managed.attach_socket;
    ops.chown(path, managed.attach_socket_uid, managed.attach_socket_gid)
        .with_context(|| {
            format!(
                "failed to chown libkrun attach socket '{}' to {}:{}",
                path.display(),
                managed.attach_socket_uid,
                managed.attach_socket_gid
            )
        })?;
    ops.chmod(path, ATTACH_SOCKET_MODE).with_context(|| {
        format!(
            "failed to chmod libkrun attach socket '{}' to {:o}",
            path.display(),
            ATTACH_SOCKET_MODE
        )
    })?;
    verify_repaired_socket(managed, ops)
}

fn verify_repaired_socket(managed: &ManagedSessionConfig, ops: &impl SocketOps) -> Result<()> {
    let metadata = ops
        .symlink_metadata(&managed.attach_socket)
        .with_context(|| {
            format!(
                "failed to re-stat repaired libkrun attach socket '{}'",
                managed.attach_socket.display()
            )
        })?;
    if !metadata.file_type().is_socket() {
        bail!(
            "repaired libkrun attach path '{}' is no longer a Unix socket",
            managed.attach_socket.display()
        );
    }
    if metadata.uid() != managed.attach_socket_uid || metadata.gid() != managed.attach_socket_gid {
        bail!(
            "repaired libkrun attach socket '{}' has owner {}:{}, expected {}:{}",
            managed.attach_socket.display(),
            metadata.uid(),
            metadata.gid(),
            managed.attach_socket_uid,
            managed.attach_socket_gid
        );
    }
    let actual_mode = metadata.permissions().mode() & 0o777;
    if actual_mode != ATTACH_SOCKET_MODE {
        bail!(
            "repaired libkrun attach socket '{}' has mode {:o}, expected {:o}",
            managed.attach_socket.display(),
            actual_mode,
            ATTACH_SOCKET_MODE
        );
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HandshakeProbeOutcome {
    Ready,
    NotReady,
}

trait HandshakeProbe {
    fn probe(&self, path: &Path, read_timeout: Duration) -> Result<HandshakeProbeOutcome>;
}

struct RealHandshakeProbe;

impl HandshakeProbe for RealHandshakeProbe {
    fn probe(&self, path: &Path, read_timeout: Duration) -> Result<HandshakeProbeOutcome> {
        let mut stream = match UnixStream::connect(path) {
            Ok(stream) => stream,
            Err(err) if is_transient_connect_error(&err) => {
                return Ok(HandshakeProbeOutcome::NotReady);
            }
            Err(err) => {
                return Err(err).with_context(|| {
                    format!(
                        "failed to connect to repaired libkrun attach socket '{}' for readiness handshake probe",
                        path.display()
                    )
                });
            }
        };
        stream
            .set_read_timeout(Some(read_timeout))
            .with_context(|| {
                format!(
                    "failed to set readiness handshake timeout on libkrun attach socket '{}'",
                    path.display()
                )
            })?;
        match read_frame(&mut stream) {
            Ok(Some(Frame::Hello { version })) if version == PROTOCOL_VERSION => {
                let _ = write_frame(&mut stream, &Frame::Detach);
                Ok(HandshakeProbeOutcome::Ready)
            }
            Ok(Some(Frame::Hello { version })) => bail!(
                "libkrun attach readiness probe received protocol version {version}, expected {PROTOCOL_VERSION}"
            ),
            Ok(Some(Frame::Busy)) => Ok(HandshakeProbeOutcome::Ready),
            Ok(Some(Frame::Error(message))) => {
                bail!("libkrun attach readiness probe failed: {message}")
            }
            Ok(Some(frame)) => {
                bail!("libkrun attach readiness probe received unexpected initial frame: {frame:?}")
            }
            Ok(None) => Ok(HandshakeProbeOutcome::NotReady),
            Err(err) if is_transient_handshake_error(&err) => Ok(HandshakeProbeOutcome::NotReady),
            Err(err) => Err(err).with_context(|| {
                format!(
                    "failed to read initial attach handshake from repaired libkrun socket '{}'",
                    path.display()
                )
            }),
        }
    }
}

fn is_transient_connect_error(err: &std::io::Error) -> bool {
    matches!(
        err.kind(),
        std::io::ErrorKind::NotFound
            | std::io::ErrorKind::ConnectionRefused
            | std::io::ErrorKind::ConnectionReset
            | std::io::ErrorKind::ConnectionAborted
    )
}

fn is_transient_handshake_error(err: &anyhow::Error) -> bool {
    err.chain().any(|cause| {
        cause.downcast_ref::<std::io::Error>().is_some_and(|io| {
            matches!(
                io.kind(),
                std::io::ErrorKind::TimedOut
                    | std::io::ErrorKind::WouldBlock
                    | std::io::ErrorKind::ConnectionReset
                    | std::io::ErrorKind::ConnectionAborted
            )
        })
    })
}

fn c_path(path: &Path) -> std::io::Result<CString> {
    CString::new(path.as_os_str().as_bytes())
        .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput))
}

fn status_description(status: i32) -> String {
    match status_exit_code(status) {
        Some(code) => format!("exited with status {code}"),
        None => "exited due to signal".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::os::unix::net::UnixListener;
    use tempfile::tempdir;

    #[test]
    fn repairs_existing_unix_socket_mode_and_owner() {
        let temp = tempdir().unwrap();
        let socket_path = temp.path().join("attach.sock");
        let _listener = UnixListener::bind(&socket_path).unwrap();
        fs::set_permissions(&socket_path, fs::Permissions::from_mode(0o755)).unwrap();
        let managed = managed_config(&socket_path);

        wait_for_managed_attach_socket_with(
            &managed,
            ReadyOptions {
                timeout: Duration::from_secs(1),
                poll: Duration::from_millis(1),
            },
            &RealSocketOps,
            &NoopIdentityScope,
            &ReadyHandshakeProbe,
            || Ok(None),
        )
        .unwrap();

        let metadata = fs::symlink_metadata(&socket_path).unwrap();
        assert!(metadata.file_type().is_socket());
        assert_eq!(metadata.uid(), unsafe { libc::getuid() });
        assert_eq!(metadata.gid(), unsafe { libc::getgid() });
        assert_eq!(metadata.permissions().mode() & 0o777, ATTACH_SOCKET_MODE);
    }

    #[test]
    fn rejects_non_socket_path() {
        let temp = tempdir().unwrap();
        let socket_path = temp.path().join("attach.sock");
        fs::write(&socket_path, b"not a socket").unwrap();
        let managed = managed_config(&socket_path);

        let err = wait_for_managed_attach_socket_with(
            &managed,
            ReadyOptions {
                timeout: Duration::from_secs(1),
                poll: Duration::from_millis(1),
            },
            &RealSocketOps,
            &NoopIdentityScope,
            &ReadyHandshakeProbe,
            || Ok(None),
        )
        .unwrap_err();

        assert!(format!("{err:#}").contains("is not a Unix socket"));
    }

    #[test]
    fn times_out_when_socket_never_appears() {
        let temp = tempdir().unwrap();
        let managed = managed_config(&temp.path().join("attach.sock"));

        let err = wait_for_managed_attach_socket_with(
            &managed,
            ReadyOptions {
                timeout: Duration::from_millis(1),
                poll: Duration::from_millis(1),
            },
            &RealSocketOps,
            &NoopIdentityScope,
            &ReadyHandshakeProbe,
            || Ok(None),
        )
        .unwrap_err();

        assert!(format!("{err:#}").contains("timed out waiting for libkrun attach socket"));
    }

    #[test]
    fn fails_fast_when_vm_worker_exits_before_socket_ready() {
        let temp = tempdir().unwrap();
        let managed = managed_config(&temp.path().join("attach.sock"));

        let err = wait_for_managed_attach_socket_with(
            &managed,
            ReadyOptions {
                timeout: Duration::from_secs(1),
                poll: Duration::from_millis(1),
            },
            &RealSocketOps,
            &NoopIdentityScope,
            &ReadyHandshakeProbe,
            || Ok(Some(0)),
        )
        .unwrap_err();

        assert!(format!("{err:#}").contains("vm-worker-exited-before-ready"));
    }

    #[test]
    fn runs_repair_inside_namespace_root_identity_scope() {
        let temp = tempdir().unwrap();
        let socket_path = temp.path().join("attach.sock");
        let _listener = UnixListener::bind(&socket_path).unwrap();
        fs::set_permissions(&socket_path, fs::Permissions::from_mode(0o755)).unwrap();
        let managed = managed_config(&socket_path);
        let scope = RecordingIdentityScope::default();

        wait_for_managed_attach_socket_with(
            &managed,
            ReadyOptions {
                timeout: Duration::from_secs(1),
                poll: Duration::from_millis(1),
            },
            &RealSocketOps,
            &scope,
            &ReadyHandshakeProbe,
            || Ok(None),
        )
        .unwrap();

        assert!(scope.entered.get());
        assert!(scope.completed.get());
    }

    #[test]
    fn waits_for_attach_protocol_hello_before_reporting_ready() {
        let temp = tempdir().unwrap();
        let socket_path = temp.path().join("attach.sock");
        let _listener = UnixListener::bind(&socket_path).unwrap();
        let managed = managed_config(&socket_path);
        let probe = SequencedHandshakeProbe::new([
            HandshakeProbeOutcome::NotReady,
            HandshakeProbeOutcome::Ready,
        ]);

        wait_for_managed_attach_socket_with(
            &managed,
            ReadyOptions {
                timeout: Duration::from_secs(1),
                poll: Duration::from_millis(1),
            },
            &RealSocketOps,
            &NoopIdentityScope,
            &probe,
            || Ok(None),
        )
        .unwrap();

        assert_eq!(probe.calls.get(), 2);
    }

    #[test]
    fn real_handshake_probe_treats_close_before_hello_as_not_ready() {
        let temp = tempdir().unwrap();
        let socket_path = temp.path().join("attach.sock");
        let listener = UnixListener::bind(&socket_path).unwrap();
        let server = thread::spawn(move || {
            let (client, _) = listener.accept().unwrap();
            drop(client);
        });

        let outcome = RealHandshakeProbe
            .probe(&socket_path, Duration::from_millis(100))
            .unwrap();

        assert_eq!(outcome, HandshakeProbeOutcome::NotReady);
        server.join().unwrap();
    }

    #[test]
    fn real_handshake_probe_reports_ready_after_hello() {
        let temp = tempdir().unwrap();
        let socket_path = temp.path().join("attach.sock");
        let listener = UnixListener::bind(&socket_path).unwrap();
        let server = thread::spawn(move || {
            let (mut client, _) = listener.accept().unwrap();
            write_frame(
                &mut client,
                &Frame::Hello {
                    version: PROTOCOL_VERSION,
                },
            )
            .unwrap();
            assert_eq!(read_frame(&mut client).unwrap(), Some(Frame::Detach));
        });

        let outcome = RealHandshakeProbe
            .probe(&socket_path, Duration::from_millis(100))
            .unwrap();

        assert_eq!(outcome, HandshakeProbeOutcome::Ready);
        server.join().unwrap();
    }

    struct ReadyHandshakeProbe;

    impl HandshakeProbe for ReadyHandshakeProbe {
        fn probe(&self, _path: &Path, _read_timeout: Duration) -> Result<HandshakeProbeOutcome> {
            Ok(HandshakeProbeOutcome::Ready)
        }
    }

    struct SequencedHandshakeProbe {
        outcomes: Vec<HandshakeProbeOutcome>,
        calls: Cell<usize>,
    }

    impl SequencedHandshakeProbe {
        fn new(outcomes: impl IntoIterator<Item = HandshakeProbeOutcome>) -> Self {
            Self {
                outcomes: outcomes.into_iter().collect(),
                calls: Cell::new(0),
            }
        }
    }

    impl HandshakeProbe for SequencedHandshakeProbe {
        fn probe(&self, _path: &Path, _read_timeout: Duration) -> Result<HandshakeProbeOutcome> {
            let call = self.calls.get();
            self.calls.set(call + 1);
            Ok(self.outcomes[call])
        }
    }

    struct NoopIdentityScope;

    impl FilesystemIdentityScope for NoopIdentityScope {
        fn with_namespace_root<T>(&self, operation: impl FnOnce() -> Result<T>) -> Result<T> {
            operation()
        }
    }

    #[derive(Default)]
    struct RecordingIdentityScope {
        entered: Cell<bool>,
        completed: Cell<bool>,
    }

    impl FilesystemIdentityScope for RecordingIdentityScope {
        fn with_namespace_root<T>(&self, operation: impl FnOnce() -> Result<T>) -> Result<T> {
            self.entered.set(true);
            let result = operation();
            if result.is_ok() {
                self.completed.set(true);
            }
            result
        }
    }

    fn managed_config(socket_path: &Path) -> ManagedSessionConfig {
        ManagedSessionConfig {
            attach_socket: socket_path.to_path_buf(),
            guest_port: 7777,
            protocol_version: 1,
            attach_socket_uid: unsafe { libc::getuid() },
            attach_socket_gid: unsafe { libc::getgid() },
            cleanup_task_rootfs_on_exit: false,
        }
    }
}
