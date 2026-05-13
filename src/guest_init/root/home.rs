use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};

use crate::guest_init::runtime::libkrun::{DEV_HOME, DEV_USER};
use crate::guest_init::{fs, process};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::guest_init) struct DevIdentity {
    pub(in crate::guest_init) uid: u32,
    pub(in crate::guest_init) gid: u32,
    pub(in crate::guest_init) home: PathBuf,
    pub(in crate::guest_init) shell: PathBuf,
}

impl DevIdentity {
    pub(in crate::guest_init) fn new(uid: u32, gid: u32, shell: PathBuf) -> Self {
        Self {
            uid,
            gid,
            home: PathBuf::from(DEV_HOME),
            shell,
        }
    }
}

pub(in crate::guest_init) fn materialize(identity: &DevIdentity) -> Result<()> {
    if !process::is_root() {
        return Ok(());
    }
    let passwd = build_passwd(identity)?;
    let group = build_group(identity)?;
    fs::write_file(Path::new("/etc/passwd"), &passwd, 0o644)
        .context("failed to materialize dynamic dev entry in /etc/passwd")?;
    fs::write_file(Path::new("/etc/group"), &group, 0o644)
        .context("failed to materialize dynamic dev entry in /etc/group")?;
    ensure_home_dirs(identity)
}

pub(in crate::guest_init) fn ensure_home_dirs(identity: &DevIdentity) -> Result<()> {
    for path in [
        identity.home.as_path(),
        Path::new("/home/dev/.local"),
        Path::new("/home/dev/.local/share"),
        Path::new("/home/dev/.local/state"),
        Path::new("/home/dev/.cache"),
        Path::new("/home/dev/.cache/tmp"),
        Path::new("/home/dev/.config"),
    ] {
        fs::create_dir_all(path)?;
        fs::chown(path, identity.uid, identity.gid)?;
    }
    fs::chmod(Path::new("/home/dev/.local/state"), 0o700)?;
    fs::chmod(Path::new("/home/dev/.cache/tmp"), 0o700)?;
    Ok(())
}

fn build_passwd(identity: &DevIdentity) -> Result<String> {
    let existing = read_without_dev(Path::new("/etc/passwd"))?;
    Ok(format!(
        "{existing}{DEV_USER}:x:{}:{}:dev user:{}:{}\n",
        identity.uid,
        identity.gid,
        identity.home.display(),
        identity.shell.display()
    ))
}

fn build_group(identity: &DevIdentity) -> Result<String> {
    let existing = read_without_dev(Path::new("/etc/group"))?;
    Ok(format!("{existing}{DEV_USER}:x:{}:\n", identity.gid))
}

fn read_without_dev(path: &Path) -> Result<String> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(err) => return Err(err).with_context(|| format!("failed to read {}", path.display())),
    };
    let mut out = String::new();
    for line in text.lines() {
        if !line.starts_with("dev:") {
            out.push_str(line);
            out.push('\n');
        }
    }
    Ok(out)
}

pub(in crate::guest_init) fn validate_host_identity(uid: u32, gid: u32) -> Result<()> {
    if uid == 0 || gid == 0 {
        bail!("libkrun host UID/GID must identify the non-root dev user, got {uid}:{gid}");
    }
    Ok(())
}

#[cfg(test)]
#[path = "home_tests.rs"]
mod tests;
