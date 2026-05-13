use anyhow::{Context, Result};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

pub(in crate::guest_init) fn create_dir_all(path: &Path) -> Result<()> {
    fs::create_dir_all(path).with_context(|| format!("failed to create {}", path.display()))
}

pub(in crate::guest_init) fn write_file(path: &Path, contents: &str, mode: u32) -> Result<()> {
    if let Some(parent) = path.parent() {
        create_dir_all(parent)?;
    }
    let tmp = path.with_extension(format!("tmp.{}", std::process::id()));
    {
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&tmp)
            .with_context(|| format!("failed to write {}", tmp.display()))?;
        file.write_all(contents.as_bytes())?;
        file.sync_all()?;
    }
    chmod(&tmp, mode)?;
    fs::rename(&tmp, path).with_context(|| {
        format!(
            "failed to replace {} with staged {}",
            path.display(),
            tmp.display()
        )
    })?;
    Ok(())
}

pub(in crate::guest_init) fn chmod(path: &Path, mode: u32) -> Result<()> {
    let permissions = fs::Permissions::from_mode(mode);
    fs::set_permissions(path, permissions)
        .with_context(|| format!("failed to chmod {:o} {}", mode, path.display()))
}

pub(in crate::guest_init) fn chown(path: &Path, uid: u32, gid: u32) -> Result<()> {
    let c_path = std::ffi::CString::new(path.as_os_str().as_encoded_bytes())?;
    let rc = unsafe { libc::chown(c_path.as_ptr(), uid, gid) };
    if rc == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
            .with_context(|| format!("failed to chown {} to {uid}:{gid}", path.display()))
    }
}
