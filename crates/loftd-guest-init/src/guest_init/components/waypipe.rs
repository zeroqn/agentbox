use anyhow::{Context, Result, anyhow, bail};
use std::fs;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::guest_init::components::home::identity::DevIdentity;
use crate::guest_init::components::rootless::runtime_dir::ensure_user_runtime_dir;
use crate::guest_init::process;

pub(in crate::guest_init) const WAYLAND_DISPLAY: &str = "loftd-waypipe-0";
const SOFTWARE_RENDERER_ENV: &[(&str, &str)] = &[
    ("LIBGL_ALWAYS_SOFTWARE", "1"),
    (
        "LIBGL_DRIVERS_PATH",
        "/usr/lib/loftd-software-renderer/lib/dri",
    ),
    (
        "__EGL_VENDOR_LIBRARY_FILENAMES",
        "/usr/lib/loftd-software-renderer/share/glvnd/egl_vendor.d/50_mesa.json",
    ),
    (
        "VK_DRIVER_FILES",
        "/usr/lib/loftd-software-renderer/share/vulkan/icd.d/lvp_icd.x86_64.json",
    ),
];
const READY_TIMEOUT: Duration = Duration::from_secs(10);
const READY_POLL: Duration = Duration::from_millis(20);

#[derive(Clone)]
pub(in crate::guest_init) struct WaypipeService {
    inner: Arc<Mutex<WaypipeServiceInner>>,
}

struct WaypipeServiceInner {
    program: std::path::PathBuf,
    port: u32,
    identity: DevIdentity,
    runtime_dir: std::path::PathBuf,
    socket_path: std::path::PathBuf,
    child: Option<Child>,
}

impl WaypipeService {
    pub(in crate::guest_init) fn reuse(&self) -> Result<()> {
        let mut inner = self.inner.lock().expect("Waypipe service lock poisoned");
        let child = inner
            .child
            .as_mut()
            .ok_or_else(|| anyhow!("Waypipe server is not running"))?;
        if let Some(status) = child.try_wait()? {
            bail!("Waypipe server exited with {status}");
        }
        if !inner.socket_path.exists() {
            bail!(
                "Waypipe display '{}' is not ready",
                inner.socket_path.display()
            );
        }
        Ok(())
    }

    pub(in crate::guest_init) fn replace(&self) -> Result<()> {
        let mut inner = self.inner.lock().expect("Waypipe service lock poisoned");
        if let Some(mut child) = inner.child.take()
            && child.try_wait()?.is_none()
        {
            child.kill().context("failed to terminate Waypipe server")?;
            child.wait().context("failed to reap Waypipe server")?;
        }
        remove_display(&inner.socket_path)?;
        let mut child = spawn_server(
            &inner.program,
            inner.port,
            &inner.identity,
            &inner.runtime_dir,
        )?;
        wait_ready(&mut child, &inner.socket_path)?;
        inner.child = Some(child);
        Ok(())
    }
}

pub(in crate::guest_init) fn start(port: u32, identity: &DevIdentity) -> Result<WaypipeService> {
    let runtime_dir = ensure_user_runtime_dir(identity)?;
    let socket_path = runtime_dir.join(WAYLAND_DISPLAY);
    remove_display(&socket_path)?;
    let program = std::path::PathBuf::from("waypipe");
    let mut child = spawn_server(&program, port, identity, &runtime_dir)?;
    wait_ready(&mut child, &socket_path)?;
    export_env(&runtime_dir);

    let inner = Arc::new(Mutex::new(WaypipeServiceInner {
        program,
        port,
        identity: identity.clone(),
        runtime_dir,
        socket_path,
        child: Some(child),
    }));
    let monitor = Arc::clone(&inner);
    thread::Builder::new()
        .name("loftd-waypipe-monitor".to_owned())
        .spawn(move || {
            loop {
                thread::sleep(READY_POLL);
                let status = monitor
                    .lock()
                    .expect("Waypipe service lock poisoned")
                    .child
                    .as_mut()
                    .map(Child::try_wait)
                    .transpose();
                match status {
                    Ok(Some(Some(status))) => {
                        eprintln!(
                            "loftd-guest-init: persistent Waypipe server exited with {status}"
                        );
                        std::process::exit(1);
                    }
                    Ok(Some(None) | None) => {}
                    Err(err) => {
                        eprintln!(
                            "loftd-guest-init: failed to wait for persistent Waypipe server: {err}"
                        );
                        std::process::exit(1);
                    }
                }
            }
        })
        .context("failed to start persistent Waypipe monitor")?;

    Ok(WaypipeService { inner })
}

fn spawn_server(
    program: &std::path::Path,
    port: u32,
    identity: &DevIdentity,
    runtime_dir: &std::path::Path,
) -> Result<Child> {
    let mut command = Command::new(program);
    command
        .args([
            "--no-gpu",
            "--vsock",
            "--socket",
            &port.to_string(),
            "--display",
            WAYLAND_DISPLAY,
            "server",
            "--",
            "sleep",
            "infinity",
        ])
        .env("XDG_RUNTIME_DIR", runtime_dir)
        .envs(SOFTWARE_RENDERER_ENV.iter().copied())
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    if process::is_root() {
        let identity = identity.clone();
        unsafe {
            command.pre_exec(move || process::apply_dev_credentials(&identity));
        }
    }
    command
        .spawn()
        .context("failed to start persistent Waypipe server")
}

fn wait_ready(child: &mut Child, socket_path: &std::path::Path) -> Result<()> {
    let deadline = Instant::now() + READY_TIMEOUT;
    loop {
        if socket_path.exists() {
            return Ok(());
        }
        if let Some(status) = child.try_wait()? {
            bail!("persistent Waypipe server exited before readiness with {status}");
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(anyhow!(
                "timed out waiting for persistent Waypipe display '{}'",
                socket_path.display()
            ));
        }
        thread::sleep(READY_POLL);
    }
}

fn remove_display(socket_path: &std::path::Path) -> Result<()> {
    match fs::remove_file(socket_path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err).with_context(|| {
            format!(
                "failed to remove stale Waypipe display '{}'",
                socket_path.display()
            )
        }),
    }
}

fn export_env(runtime_dir: &std::path::Path) {
    unsafe {
        std::env::set_var("XDG_RUNTIME_DIR", runtime_dir);
        std::env::set_var("WAYLAND_DISPLAY", WAYLAND_DISPLAY);
        for (name, value) in SOFTWARE_RENDERER_ENV {
            std::env::set_var(name, value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;

    #[test]
    fn failed_replacement_can_be_retried() {
        let dir = tempfile::tempdir().expect("tempdir");
        let program = dir.path().join("fake-waypipe");
        let pid_log = dir.path().join("pids");
        fs::write(
            &program,
            format!(
                "#!/bin/sh\necho $$ >> '{}'\nwhile [ \"$1\" != \"--display\" ]; do shift; done\ntouch \"$XDG_RUNTIME_DIR/$2\"\nexec sleep 60\n",
                pid_log.display()
            ),
        )
        .expect("write fake Waypipe");
        fs::set_permissions(&program, fs::Permissions::from_mode(0o755))
            .expect("make fake Waypipe executable");
        let identity = DevIdentity::new(
            unsafe { libc::geteuid() },
            unsafe { libc::getegid() },
            PathBuf::from("/bin/sh"),
        );
        let socket_path = dir.path().join(WAYLAND_DISPLAY);
        let mut child = spawn_server(&program, 50_427, &identity, dir.path()).expect("start");
        wait_ready(&mut child, &socket_path).expect("ready");
        let service = WaypipeService {
            inner: Arc::new(Mutex::new(WaypipeServiceInner {
                program: dir.path().join("missing-waypipe"),
                port: 50_427,
                identity,
                runtime_dir: dir.path().to_path_buf(),
                socket_path: socket_path.clone(),
                child: Some(child),
            })),
        };

        assert!(service.replace().is_err());
        assert!(service.inner.lock().expect("lock").child.is_none());
        service.inner.lock().expect("lock").program = program;
        service.replace().expect("retry replacement");
        assert!(socket_path.exists());
        let pids = fs::read_to_string(pid_log).expect("pid log");
        assert_eq!(pids.lines().count(), 2);

        let mut inner = service.inner.lock().expect("lock");
        let mut child = inner.child.take().expect("current child");
        child.kill().expect("kill child");
        child.wait().expect("reap child");
    }
}
