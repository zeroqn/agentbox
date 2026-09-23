use anyhow::{Context, Result, bail};
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;

pub(crate) struct WaypipeBroker {
    data_socket: PathBuf,
    control_socket: PathBuf,
}

impl WaypipeBroker {
    pub(crate) fn start(
        data_socket: PathBuf,
        control_socket: PathBuf,
        initial_target: Option<PathBuf>,
    ) -> Result<Self> {
        remove_stale_socket(&data_socket)?;
        remove_stale_socket(&control_socket)?;
        let data_listener = UnixListener::bind(&data_socket).with_context(|| {
            format!(
                "failed to bind Waypipe data socket '{}'",
                data_socket.display()
            )
        })?;
        let control_listener = UnixListener::bind(&control_socket).with_context(|| {
            format!(
                "failed to bind Waypipe control socket '{}'",
                control_socket.display()
            )
        })?;
        let target = Arc::new((Mutex::new(initial_target), Condvar::new()));
        let data_target = Arc::clone(&target);
        thread::spawn(move || {
            for guest in data_listener.incoming().flatten() {
                let target = Arc::clone(&data_target);
                thread::spawn(move || {
                    let (current, ready) = &*target;
                    let mut current = current.lock().expect("Waypipe target lock poisoned");
                    while current.is_none() {
                        current = ready.wait(current).expect("Waypipe target lock poisoned");
                    }
                    let path = current.clone().expect("Waypipe target should be active");
                    drop(current);
                    if let Ok(client) = UnixStream::connect(path) {
                        let _ = proxy(guest, client);
                    }
                });
            }
        });
        let update = Arc::new(Mutex::new(()));
        thread::spawn(move || {
            for mut stream in control_listener.incoming().flatten() {
                let update = Arc::clone(&update);
                let target = Arc::clone(&target);
                thread::spawn(move || {
                    let _update = update.lock().expect("Waypipe update lock poisoned");
                    let mut path = String::new();
                    if BufReader::new(&stream).read_line(&mut path).is_ok() {
                        let path = PathBuf::from(path.trim_end());
                        if path.is_absolute() {
                            let (current, ready) = &*target;
                            *current.lock().expect("Waypipe target lock poisoned") = Some(path);
                            ready.notify_all();
                            let _ = stream.write_all(b"ok\n");
                            let mut release = [0u8; 1];
                            let _ = stream.read(&mut release);
                            return;
                        }
                    }
                    let _ = stream.write_all(b"error\n");
                });
            }
        });
        Ok(Self {
            data_socket,
            control_socket,
        })
    }
}

impl Drop for WaypipeBroker {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.data_socket);
        let _ = fs::remove_file(&self.control_socket);
    }
}

pub(crate) fn update_target(control_socket: &Path, target: &Path) -> Result<UnixStream> {
    let mut stream = UnixStream::connect(control_socket).with_context(|| {
        format!(
            "failed to connect to Waypipe control socket '{}'",
            control_socket.display()
        )
    })?;
    writeln!(stream, "{}", target.display())?;
    let mut response = String::new();
    BufReader::new(&stream).read_line(&mut response)?;
    if response == "ok\n" {
        Ok(stream)
    } else {
        bail!("Waypipe broker rejected target update")
    }
}

fn proxy(left: UnixStream, right: UnixStream) -> Result<()> {
    let mut left_read = left.try_clone()?;
    let mut right_write = right.try_clone()?;
    let forward = thread::spawn(move || std::io::copy(&mut left_read, &mut right_write));
    let mut right_read = right;
    let mut left_write = left;
    let _ = std::io::copy(&mut right_read, &mut left_write);
    let _ = forward.join();
    Ok(())
}

fn remove_stale_socket(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => {
            Err(err).with_context(|| format!("failed to remove stale socket '{}'", path.display()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn guest_connection_waits_until_target_is_activated() {
        let dir = tempfile::tempdir().expect("tempdir");
        let data = dir.path().join("data.sock");
        let control = dir.path().join("control.sock");
        let target = dir.path().join("target.sock");
        let _broker = WaypipeBroker::start(data.clone(), control.clone(), None).expect("broker");
        let mut guest = UnixStream::connect(data).expect("guest connection");
        guest
            .set_read_timeout(Some(Duration::from_millis(50)))
            .expect("timeout");
        let mut byte = [0u8; 1];
        assert!(matches!(
            guest.read(&mut byte),
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock || err.kind() == std::io::ErrorKind::TimedOut
        ));

        let listener = UnixListener::bind(&target).expect("target listener");
        let activation = update_target(&control, &target).expect("activate target");
        let (mut client, _) = listener.accept().expect("target connection");
        client.write_all(b"x").expect("target write");
        guest
            .set_read_timeout(Some(Duration::from_secs(1)))
            .expect("timeout");
        guest.read_exact(&mut byte).expect("guest read");
        assert_eq!(byte, [b'x']);
        drop(activation);
    }
}
