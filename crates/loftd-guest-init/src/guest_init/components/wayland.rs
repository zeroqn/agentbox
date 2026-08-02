use anyhow::{Context, Result, anyhow};
use std::fs;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};

use crate::guest_init::components::home::identity::DevIdentity;
use crate::guest_init::components::rootless::runtime_dir::ensure_user_runtime_dir;
use crate::guest_init::fs as guest_fs;
use crate::guest_init::process;

pub(in crate::guest_init) const WAYLAND_DISPLAY: &str = "wayland-0";
pub(in crate::guest_init) const PROXY_BIN: &str = "wl-cross-domain-proxy";
const DRI_DIR: &str = "/dev/dri";
const ROOT_UID: u32 = 0;
const VIDEO_GID: u32 = 44;
const RENDER_GID: u32 = 107;

pub(in crate::guest_init) fn start_if_enabled(
    wayland_enabled: bool,
    gpu_drm_enabled: bool,
    identity: &DevIdentity,
) -> Result<Option<Child>> {
    prepare_drm_devices_for_start(wayland_enabled, gpu_drm_enabled, Path::new(DRI_DIR))?;
    if !wayland_enabled {
        return Ok(None);
    }
    let runtime_dir = ensure_user_runtime_dir(identity)?;
    export_guest_env(&runtime_dir);
    let child = spawn_proxy(&runtime_dir, WAYLAND_DISPLAY, identity)
        .context("failed to start guest Wayland cross-domain proxy")?;
    Ok(Some(child))
}

fn prepare_drm_devices_for_start(
    wayland_enabled: bool,
    gpu_drm_enabled: bool,
    dri_dir: &Path,
) -> Result<()> {
    prepare_drm_devices_for_start_with(
        wayland_enabled,
        gpu_drm_enabled,
        dri_dir,
        &mut |path, permissions| {
            guest_fs::chown(path, ROOT_UID, permissions.gid)?;
            guest_fs::chmod(path, permissions.mode)
        },
    )
}

fn prepare_drm_devices_for_start_with(
    wayland_enabled: bool,
    gpu_drm_enabled: bool,
    dri_dir: &Path,
    apply: &mut impl FnMut(&Path, DrmDevicePermissions) -> Result<()>,
) -> Result<()> {
    if wayland_enabled || gpu_drm_enabled {
        prepare_drm_devices_under_with(dri_dir, apply)?;
    }
    Ok(())
}

fn export_guest_env(runtime_dir: &Path) {
    unsafe {
        std::env::set_var("XDG_RUNTIME_DIR", runtime_dir);
        std::env::set_var("WAYLAND_DISPLAY", WAYLAND_DISPLAY);
    }
}

fn spawn_proxy(runtime_dir: &Path, wayland_display: &str, identity: &DevIdentity) -> Result<Child> {
    let socket_path = runtime_dir.join(wayland_display);
    if socket_path.exists() {
        fs::remove_file(&socket_path).with_context(|| {
            format!(
                "failed to remove stale guest Wayland socket '{}'",
                socket_path.display()
            )
        })?;
    }
    spawn_proxy_command(runtime_dir, wayland_display, identity, process::uid())
        .spawn()
        .with_context(|| anyhow!("failed to spawn {PROXY_BIN}"))
}

fn proxy_credential_plan(
    starting_uid: u32,
    identity: &DevIdentity,
) -> Option<[process::CredentialOperation; 3]> {
    (starting_uid == ROOT_UID).then(|| process::credential_plan(identity))
}

fn spawn_proxy_command(
    runtime_dir: &Path,
    wayland_display: &str,
    identity: &DevIdentity,
    starting_uid: u32,
) -> Command {
    let mut command = Command::new(PROXY_BIN);
    command
        .arg("--socket-name")
        .arg(wayland_display)
        .env("XDG_RUNTIME_DIR", runtime_dir)
        .env("WAYLAND_DISPLAY", wayland_display)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit());
    if proxy_credential_plan(starting_uid, identity).is_some() {
        let identity = identity.clone();
        unsafe {
            command.pre_exec(move || process::apply_dev_credentials(&identity, Default::default()));
        }
    }
    command
}

fn prepare_drm_devices_under_with(
    dri_dir: &Path,
    apply: &mut impl FnMut(&Path, DrmDevicePermissions) -> Result<()>,
) -> Result<()> {
    let Ok(entries) = fs::read_dir(dri_dir) else {
        return Ok(());
    };
    for entry in entries {
        let entry =
            entry.with_context(|| format!("failed to read entry under {}", dri_dir.display()))?;
        let path = entry.path();
        if let Some(permissions) = drm_device_permissions(&path) {
            apply(&path, permissions)?;
        }
    }
    Ok(())
}

#[derive(Debug, PartialEq, Eq)]
struct DrmDevicePermissions {
    gid: u32,
    mode: u32,
}

fn drm_device_permissions(path: &Path) -> Option<DrmDevicePermissions> {
    let name = path.file_name()?.to_str()?;
    if name.starts_with("renderD") {
        Some(DrmDevicePermissions {
            gid: RENDER_GID,
            mode: 0o666,
        })
    } else if name.starts_with("card") {
        Some(DrmDevicePermissions {
            gid: VIDEO_GID,
            mode: 0o660,
        })
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proxy_constants_match_guest_env_contract() {
        assert_eq!(WAYLAND_DISPLAY, "wayland-0");
        assert_eq!(PROXY_BIN, "wl-cross-domain-proxy");
    }

    #[test]
    fn proxy_command_forces_exported_socket_name_and_dev_credentials() {
        let identity = DevIdentity::new(1000, 1000, "/bin/sh".into());
        let command = spawn_proxy_command(
            Path::new("/run/user/1000"),
            WAYLAND_DISPLAY,
            &identity,
            ROOT_UID,
        );

        let args: Vec<_> = command.get_args().collect();
        assert_eq!(args, ["--socket-name", WAYLAND_DISPLAY]);
        assert_eq!(
            command
                .get_envs()
                .find(|(key, _)| *key == std::ffi::OsStr::new("XDG_RUNTIME_DIR")),
            Some((
                std::ffi::OsStr::new("XDG_RUNTIME_DIR"),
                Some(std::ffi::OsStr::new("/run/user/1000"))
            ))
        );
        assert_eq!(
            command
                .get_envs()
                .find(|(key, _)| *key == std::ffi::OsStr::new("WAYLAND_DISPLAY")),
            Some((
                std::ffi::OsStr::new("WAYLAND_DISPLAY"),
                Some(std::ffi::OsStr::new(WAYLAND_DISPLAY))
            ))
        );
        assert_eq!(
            proxy_credential_plan(ROOT_UID, &identity),
            Some(process::credential_plan(&identity))
        );
        assert_eq!(proxy_credential_plan(identity.uid, &identity), None);
    }

    #[test]
    fn drm_preparation_runs_without_wayland_when_gpu_drm_is_enabled() {
        let temp = tempfile::tempdir().expect("temporary directory should be created");
        let card = temp.path().join("card0");
        let render = temp.path().join("renderD128");
        let unrelated = temp.path().join("by-path");
        fs::write(&card, "").expect("card node should be created");
        fs::write(&render, "").expect("render node should be created");
        fs::write(&unrelated, "").expect("unrelated entry should be created");
        let mut applied = Vec::new();

        prepare_drm_devices_for_start_with(false, true, temp.path(), &mut |path, permissions| {
            applied.push((path.to_path_buf(), permissions));
            Ok(())
        })
        .expect("DRM preparation should succeed without Wayland");

        applied.sort_by(|left, right| left.0.cmp(&right.0));
        assert_eq!(
            applied,
            [
                (
                    card,
                    DrmDevicePermissions {
                        gid: VIDEO_GID,
                        mode: 0o660,
                    },
                ),
                (
                    render,
                    DrmDevicePermissions {
                        gid: RENDER_GID,
                        mode: 0o666,
                    },
                ),
            ]
        );
    }

    #[test]
    fn drm_device_permissions_match_libkrun_gpu_nodes() {
        assert_eq!(
            drm_device_permissions(Path::new("/dev/dri/renderD128")),
            Some(DrmDevicePermissions {
                gid: RENDER_GID,
                mode: 0o666,
            })
        );
        assert_eq!(
            drm_device_permissions(Path::new("/dev/dri/card0")),
            Some(DrmDevicePermissions {
                gid: VIDEO_GID,
                mode: 0o660,
            })
        );
        assert_eq!(drm_device_permissions(Path::new("/dev/dri/by-path")), None);
    }
}
