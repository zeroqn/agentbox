use anyhow::{Context, Result};
use std::fs;
use std::io;
use std::net::Shutdown;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use crate::guest_init::components::home::identity::DevIdentity;
use crate::guest_init::components::rootless::runtime_dir::ensure_user_runtime_dir;
use crate::guest_init::runtime::vsock;

const SOCKET_NAME: &str = "loftd-pulse";
const READY_TIMEOUT: Duration = Duration::from_secs(10);
const READY_POLL: Duration = Duration::from_millis(20);

pub(in crate::guest_init) fn server_value(identity: &DevIdentity) -> String {
    format!("unix:/run/user/{}/{}", identity.uid, SOCKET_NAME)
}

pub(in crate::guest_init) fn start(port: u32, identity: &DevIdentity) -> Result<()> {
    let runtime_dir = ensure_user_runtime_dir(identity)?;
    let socket = runtime_dir.join(SOCKET_NAME);
    remove_socket(&socket)?;
    let current_exe = std::env::current_exe().context("failed to resolve guest-init executable")?;
    let mut child = Command::new(current_exe)
        .args([
            "internal",
            "pulse",
            &port.to_string(),
            &identity.uid.to_string(),
            &identity.gid.to_string(),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .context("failed to spawn guest Pulse bridge")?;
    wait_ready(&mut child, &socket)
}

pub(in crate::guest_init) fn run(port: u32, uid: u32, gid: u32) -> Result<()> {
    let socket = PathBuf::from(format!("/run/user/{uid}/{SOCKET_NAME}"));
    remove_socket(&socket)?;
    let listener = UnixListener::bind(&socket)
        .with_context(|| format!("failed to bind guest Pulse socket '{}'", socket.display()))?;
    crate::guest_init::fs::chown(&socket, uid, gid)?;
    crate::guest_init::fs::chmod(&socket, 0o600)?;

    for client in listener.incoming() {
        match client {
            Ok(client) => {
                thread::spawn(move || {
                    if let Err(err) = relay(client, port) {
                        eprintln!("loftd-guest-init: Pulse bridge connection failed: {err:#}");
                    }
                });
            }
            Err(err) => return Err(err).context("guest Pulse bridge listener failed"),
        }
    }
    Ok(())
}

fn relay(mut client: UnixStream, port: u32) -> Result<()> {
    let mut host = vsock::connect_host(port)?;
    let mut client_read = client.try_clone()?;
    let mut host_write = host.try_clone()?;
    let forward = thread::spawn(move || io::copy(&mut client_read, &mut host_write));
    let reverse = io::copy(&mut host, &mut client);
    let _ = client.shutdown(Shutdown::Write);
    let _ = forward.join();
    reverse?;
    Ok(())
}

fn wait_ready(child: &mut std::process::Child, socket: &Path) -> Result<()> {
    let deadline = Instant::now() + READY_TIMEOUT;
    loop {
        if socket.exists() {
            return Ok(());
        }
        if let Some(status) = child.try_wait()? {
            anyhow::bail!("guest Pulse bridge exited before readiness with {status}");
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            anyhow::bail!(
                "timed out waiting for guest Pulse socket '{}'",
                socket.display()
            );
        }
        thread::sleep(READY_POLL);
    }
}

fn remove_socket(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err).with_context(|| {
            format!(
                "failed to remove stale guest Pulse socket '{}'",
                path.display()
            )
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn server_value_uses_dev_runtime_directory() {
        let identity = DevIdentity::new(1234, 1235, PathBuf::from("/bin/sh"));
        assert_eq!(server_value(&identity), "unix:/run/user/1234/loftd-pulse");
    }
}
