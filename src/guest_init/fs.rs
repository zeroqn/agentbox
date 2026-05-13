use anyhow::{Context, Result};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

pub(in crate::guest_init) fn create_dir_all(path: &Path) -> Result<()> {
    fs::create_dir_all(path).with_context(|| format!("failed to create {}", path.display()))
}

pub(in crate::guest_init) fn write_file(path: &Path, contents: &str, mode: u32) -> Result<()> {
    write_file_with_rename(path, contents, mode, |tmp, target| fs::rename(tmp, target))
}

fn write_file_with_rename(
    path: &Path,
    contents: &str,
    mode: u32,
    rename: impl FnOnce(&Path, &Path) -> std::io::Result<()>,
) -> Result<()> {
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
    if let Err(err) = rename(&tmp, path) {
        if is_resource_busy(&err) {
            overwrite_busy_file(path, contents, mode).with_context(|| {
                format!(
                    "failed to overwrite busy {} after staged replacement from {} failed: {err}",
                    path.display(),
                    tmp.display()
                )
            })?;
            let _ = fs::remove_file(&tmp);
        } else {
            return Err(err).with_context(|| {
                format!(
                    "failed to replace {} with staged {}",
                    path.display(),
                    tmp.display()
                )
            });
        }
    }
    Ok(())
}

fn is_resource_busy(err: &std::io::Error) -> bool {
    err.kind() == std::io::ErrorKind::ResourceBusy || err.raw_os_error() == Some(libc::EBUSY)
}

fn overwrite_busy_file(path: &Path, contents: &str, mode: u32) -> Result<()> {
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(path)
        .with_context(|| format!("failed to write {}", path.display()))?;
    file.write_all(contents.as_bytes())?;
    file.sync_all()?;
    drop(file);
    chmod(path, mode)
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

#[cfg(test)]
mod tests {
    use super::write_file_with_rename;
    use std::fs;
    use std::io;
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;

    #[test]
    fn write_file_falls_back_to_in_place_overwrite_when_rename_is_busy() {
        let dir = tempfile::tempdir().expect("tempdir should be created");
        let path = dir.path().join("resolv.conf");
        fs::write(&path, "nameserver 8.8.8.8\n").expect("seed file should be written");

        write_file_with_rename(&path, "nameserver 169.254.1.1\n", 0o640, |_, _| {
            Err(io::Error::from_raw_os_error(libc::EBUSY))
        })
        .expect("busy destination should be overwritten in place");

        assert_eq!(
            fs::read_to_string(&path).expect("destination should be readable"),
            "nameserver 169.254.1.1\n"
        );
        assert_eq!(
            fs::metadata(&path)
                .expect("destination metadata should be readable")
                .permissions()
                .mode()
                & 0o777,
            0o640
        );
        assert!(
            !staged_path(&path).exists(),
            "staged file should be removed after busy fallback"
        );
    }

    #[test]
    fn write_file_keeps_rename_errors_for_non_busy_failures() {
        let dir = tempfile::tempdir().expect("tempdir should be created");
        let path = dir.path().join("resolv.conf");

        let err = write_file_with_rename(&path, "nameserver 169.254.1.1\n", 0o644, |_, _| {
            Err(io::Error::new(io::ErrorKind::PermissionDenied, "nope"))
        })
        .expect_err("non-busy rename failures should still fail");

        assert!(
            format!("{err:#}").contains("failed to replace"),
            "error should preserve staged replacement context: {err:#}"
        );
        assert!(
            staged_path(&path).exists(),
            "non-busy failures should preserve the staged file for diagnosis"
        );
    }

    fn staged_path(path: &Path) -> std::path::PathBuf {
        path.with_extension(format!("tmp.{}", std::process::id()))
    }
}
