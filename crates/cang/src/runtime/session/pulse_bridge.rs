use anyhow::{Context, Result};
use std::fs;
use std::io;
use std::net::{Ipv4Addr, Shutdown, TcpStream};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::thread;

pub(crate) struct PulseBridge {
    socket: PathBuf,
}

impl PulseBridge {
    pub(crate) fn start(socket: PathBuf, host_port: u16) -> Result<Self> {
        remove_socket(&socket)?;
        let listener = UnixListener::bind(&socket).with_context(|| {
            format!("failed to bind Pulse bridge socket '{}'", socket.display())
        })?;

        thread::spawn(move || {
            for guest in listener.incoming() {
                match guest {
                    Ok(guest) => {
                        thread::spawn(move || {
                            if let Err(err) = relay(guest, host_port) {
                                tracing::debug!(host_port, error = %err, "Pulse bridge connection failed");
                            }
                        });
                    }
                    Err(err) => {
                        tracing::debug!(error = %err, "Pulse bridge listener stopped");
                        break;
                    }
                }
            }
        });

        Ok(Self { socket })
    }
}

impl Drop for PulseBridge {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.socket);
    }
}

fn relay(mut guest: UnixStream, host_port: u16) -> Result<()> {
    let mut host = TcpStream::connect((Ipv4Addr::LOCALHOST, host_port))
        .with_context(|| format!("failed to connect host Pulse listener 127.0.0.1:{host_port}"))?;
    let mut guest_read = guest.try_clone()?;
    let mut host_write = host.try_clone()?;
    let forward = thread::spawn(move || {
        let result = io::copy(&mut guest_read, &mut host_write);
        let _ = host_write.shutdown(Shutdown::Write);
        result
    });

    let reverse = io::copy(&mut host, &mut guest);
    let _ = guest.shutdown(Shutdown::Write);
    let _ = forward.join();
    reverse?;
    Ok(())
}

fn remove_socket(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err).with_context(|| {
            format!(
                "failed to remove stale Pulse bridge socket '{}'",
                path.display()
            )
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::time::Duration;

    #[test]
    fn bridge_proxies_multiple_connections_bidirectionally() {
        let dir = tempfile::tempdir().expect("tempdir");
        let socket = dir.path().join("pulse.sock");
        let host = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("host listener");
        let port = host.local_addr().expect("host address").port();
        let bridge = PulseBridge::start(socket.clone(), port).expect("bridge");

        for byte in *b"ab" {
            let mut guest = UnixStream::connect(&socket).expect("guest connection");
            guest
                .set_read_timeout(Some(Duration::from_secs(1)))
                .expect("read timeout");
            let (mut server, _) = host.accept().expect("host connection");
            guest.write_all(&[byte]).expect("guest write");
            let mut received = [0];
            server.read_exact(&mut received).expect("host read");
            assert_eq!(received, [byte]);
            server.write_all(&[byte + 1]).expect("host write");
            guest.read_exact(&mut received).expect("guest read");
            assert_eq!(received, [byte + 1]);
        }

        drop(bridge);
        assert!(!socket.exists());
    }

    #[test]
    fn unavailable_host_listener_does_not_stop_bridge() {
        let dir = tempfile::tempdir().expect("tempdir");
        let socket = dir.path().join("pulse.sock");
        let reserved = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("reserved listener");
        let port = reserved.local_addr().expect("reserved address").port();
        drop(reserved);
        let _bridge = PulseBridge::start(socket.clone(), port).expect("bridge");

        let mut failed = UnixStream::connect(&socket).expect("first guest connection");
        failed
            .set_read_timeout(Some(Duration::from_secs(1)))
            .expect("read timeout");
        let mut byte = [0];
        assert_eq!(failed.read(&mut byte).expect("closed connection"), 0);

        let host = TcpListener::bind((Ipv4Addr::LOCALHOST, port)).expect("host listener");
        let mut guest = UnixStream::connect(&socket).expect("second guest connection");
        let (mut server, _) = host.accept().expect("host connection");
        guest.write_all(b"x").expect("guest write");
        server.read_exact(&mut byte).expect("host read");
        assert_eq!(byte, [b'x']);
    }
}
