use anyhow::{Context, Result, anyhow, bail};
use std::fs;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
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

pub(in crate::guest_init) fn start(
    port: u32,
    identity: &DevIdentity,
) -> Result<thread::JoinHandle<()>> {
    let runtime_dir = ensure_user_runtime_dir(identity)?;
    let socket_path = runtime_dir.join(WAYLAND_DISPLAY);
    match fs::remove_file(&socket_path) {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => {
            return Err(err).with_context(|| {
                format!(
                    "failed to remove stale Waypipe display '{}'",
                    socket_path.display()
                )
            });
        }
    }

    let mut command = Command::new("waypipe");
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
        .env("XDG_RUNTIME_DIR", &runtime_dir)
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
    let mut child = command
        .spawn()
        .context("failed to start persistent Waypipe server")?;
    let deadline = Instant::now() + READY_TIMEOUT;
    loop {
        if socket_path.exists() {
            unsafe {
                std::env::set_var("XDG_RUNTIME_DIR", &runtime_dir);
                std::env::set_var("WAYLAND_DISPLAY", WAYLAND_DISPLAY);
                for (name, value) in SOFTWARE_RENDERER_ENV {
                    std::env::set_var(name, value);
                }
            }
            return thread::Builder::new()
                .name("loftd-waypipe-monitor".to_owned())
                .spawn(move || match child.wait() {
                    Ok(status) => {
                        eprintln!(
                            "loftd-guest-init: persistent Waypipe server exited with {status}"
                        );
                        std::process::exit(1);
                    }
                    Err(err) => {
                        eprintln!(
                            "loftd-guest-init: failed to wait for persistent Waypipe server: {err}"
                        );
                        std::process::exit(1);
                    }
                })
                .context("failed to start persistent Waypipe monitor");
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
