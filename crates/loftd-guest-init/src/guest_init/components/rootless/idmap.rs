use anyhow::{Context, Result, anyhow, bail};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::guest_init::command;
use crate::guest_init::components::env::GuestPermissions;
use crate::guest_init::components::home::identity::DevIdentity;
use crate::guest_init::fs;
use crate::guest_init::process;

const SUBID_START: u32 = 100_000;
const SUBID_COUNT: u32 = 65_536;
pub(in crate::guest_init) const WRAPPER_BIN_DIR: &str = "/run/loftd/wrappers/bin";

pub(in crate::guest_init) fn prepare(
    identity: &DevIdentity,
    permissions: GuestPermissions,
) -> Result<()> {
    materialize_subid_files(identity)?;
    install_helper("newuidmap")?;
    install_helper("newgidmap")?;
    install_granted_helper(permissions)?;
    seal_wrapper_tree()
}

fn materialize_subid_files(identity: &DevIdentity) -> Result<()> {
    reject_subid_overlap(0, "root")?;
    reject_subid_overlap(identity.uid, "dev-uid")?;
    reject_subid_overlap(identity.gid, "dev-gid")?;
    for path in [Path::new("/etc/subuid"), Path::new("/etc/subgid")] {
        let existing = std::fs::read_to_string(path).unwrap_or_default();
        let mut contents = String::new();
        for line in existing.lines() {
            if !line.starts_with("dev:") {
                contents.push_str(line);
                contents.push('\n');
            }
        }
        contents.push_str(&format!("dev:{SUBID_START}:{SUBID_COUNT}\n"));
        fs::write_file(path, &contents, 0o644).with_context(|| {
            format!(
                "failed to materialize {} for rootless container runtimes",
                path.display()
            )
        })?;
    }
    Ok(())
}

fn reject_subid_overlap(candidate: u32, name: &str) -> Result<()> {
    let end = SUBID_START + SUBID_COUNT - 1;
    if (SUBID_START..=end).contains(&candidate) {
        bail!("subordinate ID range {SUBID_START}:{SUBID_COUNT} overlaps {name} id {candidate}");
    }
    Ok(())
}

fn install_granted_helper(permissions: GuestPermissions) -> Result<()> {
    let wrapper_dir = Path::new(WRAPPER_BIN_DIR);
    fs::create_dir_all(wrapper_dir)?;
    let dst = wrapper_dir.join("loftd-granted");
    let src = source_helper_on_path("loftd-granted", wrapper_dir)?;
    command::run(
        "install",
        &[
            "-m",
            "0555",
            "-o",
            "0",
            "-g",
            "0",
            path_str(&src)?,
            path_str(&dst)?,
        ],
    )
    .context("failed to install root-owned loftd-granted helper")?;

    let capabilities = process::workload_capability_plan(permissions);
    set_file_capabilities(&dst, capabilities.as_slice())?;
    verify_granted_helper(&dst, capabilities.as_slice())
}

fn file_capability_value(capabilities: &[u32]) -> [u8; 20] {
    const VFS_CAP_REVISION_2: u32 = 0x0200_0000;
    const VFS_CAP_FLAGS_EFFECTIVE: u32 = 1;

    let mut permitted = [0_u32; 2];
    for capability in capabilities {
        permitted[(*capability / 32) as usize] |= 1 << (*capability % 32);
    }
    let mut value = [0_u8; 20];
    value[0..4].copy_from_slice(&(VFS_CAP_REVISION_2 | VFS_CAP_FLAGS_EFFECTIVE).to_le_bytes());
    value[4..8].copy_from_slice(&permitted[0].to_le_bytes());
    value[12..16].copy_from_slice(&permitted[1].to_le_bytes());
    value
}

fn set_file_capabilities(path: &Path, capabilities: &[u32]) -> Result<()> {
    let value = file_capability_value(capabilities);
    let path = std::ffi::CString::new(path.as_os_str().as_encoded_bytes())?;
    if unsafe {
        libc::setxattr(
            path.as_ptr(),
            c"security.capability".as_ptr(),
            value.as_ptr().cast(),
            value.len(),
            0,
        )
    } != 0
    {
        return Err(std::io::Error::last_os_error())
            .context("failed to set loftd-granted file capabilities");
    }
    Ok(())
}

fn verify_granted_helper(path: &Path, capabilities: &[u32]) -> Result<()> {
    let metadata = path
        .metadata()
        .with_context(|| format!("failed to stat {}", path.display()))?;
    if !metadata.is_file()
        || metadata.uid() != 0
        || metadata.gid() != 0
        || metadata.permissions().mode() & 0o7777 != 0o555
    {
        bail!("unexpected loftd-granted ownership or mode");
    }

    let path = std::ffi::CString::new(path.as_os_str().as_encoded_bytes())?;
    let mut value = [0_u8; 24];
    let size = unsafe {
        libc::getxattr(
            path.as_ptr(),
            c"security.capability".as_ptr(),
            value.as_mut_ptr().cast(),
            value.len(),
        )
    };
    if size < 0 {
        return Err(std::io::Error::last_os_error())
            .context("failed to verify loftd-granted file capabilities");
    }

    let expected = file_capability_value(capabilities);
    if size != expected.len() as isize || value[..expected.len()] != expected {
        bail!("installed loftd-granted capabilities differ from the authorized set");
    }
    Ok(())
}

fn seal_wrapper_tree() -> Result<()> {
    let wrappers = std::ffi::CString::new("/run/loftd/wrappers")?;
    if unsafe {
        libc::mount(
            wrappers.as_ptr(),
            wrappers.as_ptr(),
            std::ptr::null(),
            libc::MS_BIND,
            std::ptr::null(),
        )
    } != 0
    {
        return Err(std::io::Error::last_os_error()).context("failed to bind-mount loftd wrappers");
    }
    if unsafe {
        libc::mount(
            std::ptr::null(),
            wrappers.as_ptr(),
            std::ptr::null(),
            libc::MS_BIND | libc::MS_REMOUNT | libc::MS_RDONLY,
            std::ptr::null(),
        )
    } != 0
    {
        return Err(std::io::Error::last_os_error())
            .context("failed to remount loftd wrappers read-only");
    }
    Ok(())
}

fn install_helper(name: &str) -> Result<()> {
    let wrapper_dir = Path::new(WRAPPER_BIN_DIR);
    fs::create_dir_all(wrapper_dir)?;
    let dst = wrapper_dir.join(name);
    let src = source_helper_on_path(name, wrapper_dir)?;
    if src == dst || installed_helper_is_ready(&dst) {
        return Ok(());
    }
    let install_result = command::run(
        "install",
        &[
            "-m",
            "4755",
            "-o",
            "0",
            "-g",
            "0",
            path_str(&src)?,
            path_str(&dst)?,
        ],
    );
    match install_result {
        Ok(()) => Ok(()),
        Err(err) => {
            if wait_for_installed_helper_ready(&dst) {
                Ok(())
            } else {
                Err(err)
                    .with_context(|| format!("failed to install root-owned setuid {name} helper"))
            }
        }
    }
}

fn wait_for_installed_helper_ready(path: &Path) -> bool {
    let deadline = Instant::now() + Duration::from_secs(1);
    loop {
        if installed_helper_is_ready(path) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn installed_helper_is_ready(path: &Path) -> bool {
    path.metadata()
        .map(|metadata| {
            helper_metadata_is_ready(
                metadata.is_file(),
                metadata.permissions().mode(),
                metadata.uid(),
                metadata.gid(),
            )
        })
        .unwrap_or(false)
}

fn helper_metadata_is_ready(is_file: bool, mode: u32, uid: u32, gid: u32) -> bool {
    is_file && uid == 0 && gid == 0 && mode & 0o111 != 0 && mode & 0o4000 != 0
}

fn source_helper_on_path(name: &str, wrapper_dir: &Path) -> Result<PathBuf> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    source_helper_in_path(name, wrapper_dir, &path)
}

fn source_helper_in_path(
    name: &str,
    wrapper_dir: &Path,
    path: &std::ffi::OsStr,
) -> Result<PathBuf> {
    std::env::split_paths(path)
        .filter(|dir| dir != wrapper_dir)
        .map(|dir| dir.join(name))
        .find(|candidate| command::is_executable(candidate))
        .or_else(|| {
            let existing = wrapper_dir.join(name);
            installed_helper_is_ready(&existing).then_some(existing)
        })
        .ok_or_else(|| anyhow!("required tool '{name}' is not available on PATH"))
}

fn path_str(path: &Path) -> Result<&str> {
    path.to_str()
        .ok_or_else(|| anyhow!("path is not valid UTF-8: {}", path.display()))
}

#[cfg(test)]
#[path = "idmap_tests.rs"]
mod tests;
