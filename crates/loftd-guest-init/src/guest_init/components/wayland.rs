use anyhow::{Context, Result, anyhow};
use std::ffi::CString;
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use crate::guest_init::components::home::identity::DevIdentity;

pub(in crate::guest_init) const WAYLAND_DISPLAY: &str = "wayland-0";
pub(in crate::guest_init) const PROXY_BIN: &str = "wl-cross-domain-proxy";

pub(in crate::guest_init) fn start_if_enabled(
    enabled: bool,
    identity: &DevIdentity,
) -> Result<Option<Child>> {
    if !enabled {
        return Ok(None);
    }
    let runtime_dir = runtime_dir(identity.uid);
    prepare_runtime_dir(&runtime_dir, identity)?;
    export_guest_env(&runtime_dir);
    let child = spawn_proxy(&runtime_dir, WAYLAND_DISPLAY)
        .context("failed to start guest Wayland cross-domain proxy")?;
    Ok(Some(child))
}

fn runtime_dir(uid: u32) -> PathBuf {
    PathBuf::from(format!("/run/user/{uid}"))
}

fn prepare_runtime_dir(path: &Path, identity: &DevIdentity) -> Result<()> {
    fs::create_dir_all(path).with_context(|| {
        format!(
            "failed to create guest XDG_RUNTIME_DIR '{}'",
            path.display()
        )
    })?;
    let c_path = CString::new(path.as_os_str().as_bytes())?;
    let rc = unsafe { libc::chown(c_path.as_ptr(), identity.uid, identity.gid) };
    if rc != 0 {
        return Err(std::io::Error::last_os_error()).with_context(|| {
            format!("failed to chown guest XDG_RUNTIME_DIR '{}'", path.display())
        });
    }
    let mut permissions = fs::metadata(path)
        .with_context(|| format!("failed to stat guest XDG_RUNTIME_DIR '{}'", path.display()))?
        .permissions();
    use std::os::unix::fs::PermissionsExt;
    permissions.set_mode(0o700);
    fs::set_permissions(path, permissions)
        .with_context(|| format!("failed to chmod guest XDG_RUNTIME_DIR '{}'", path.display()))
}

fn export_guest_env(runtime_dir: &Path) {
    unsafe {
        std::env::set_var("XDG_RUNTIME_DIR", runtime_dir);
        std::env::set_var("WAYLAND_DISPLAY", WAYLAND_DISPLAY);
    }
}

fn spawn_proxy(runtime_dir: &Path, wayland_display: &str) -> Result<Child> {
    let socket_path = runtime_dir.join(wayland_display);
    if socket_path.exists() {
        fs::remove_file(&socket_path).with_context(|| {
            format!(
                "failed to remove stale guest Wayland socket '{}'",
                socket_path.display()
            )
        })?;
    }
    Command::new(PROXY_BIN)
        .env("XDG_RUNTIME_DIR", runtime_dir)
        .env("WAYLAND_DISPLAY", wayland_display)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .with_context(|| anyhow!("failed to spawn {PROXY_BIN}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_dir_uses_guest_uid() {
        assert_eq!(runtime_dir(1000), PathBuf::from("/run/user/1000"));
    }

    #[test]
    fn proxy_constants_match_guest_env_contract() {
        assert_eq!(WAYLAND_DISPLAY, "wayland-0");
        assert_eq!(PROXY_BIN, "wl-cross-domain-proxy");
    }
}
