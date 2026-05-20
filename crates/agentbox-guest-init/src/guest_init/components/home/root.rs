use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use crate::guest_init::components::env::DEV_USER;
use crate::guest_init::components::home::identity::DevIdentity;
use crate::guest_init::{fs, process};

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
    ensure_home_dirs(identity)?;
    crate::guest_init::components::shell::fish::materialize_configs(identity)
}

pub(in crate::guest_init) fn ensure_home_dirs(identity: &DevIdentity) -> Result<()> {
    for path in home_dirs(identity) {
        fs::create_dir_all(&path)?;
        fs::chown(&path, identity.uid, identity.gid)?;
    }
    let cache_nix_dir = nix_cache_dir(identity);
    fs::create_dir_all(&cache_nix_dir)?;
    fs::chown_tree_skipping_symlinks(&cache_nix_dir, identity.uid, identity.gid)?;
    fs::chmod(&identity.home.join(".local/state"), 0o700)?;
    fs::chmod(&identity.home.join(".cache/tmp"), 0o700)?;
    Ok(())
}

fn home_dirs(identity: &DevIdentity) -> [PathBuf; 7] {
    [
        identity.home.clone(),
        identity.home.join(".local"),
        identity.home.join(".local/share"),
        identity.home.join(".local/state"),
        identity.home.join(".cache"),
        identity.home.join(".cache/tmp"),
        identity.home.join(".config"),
    ]
}

fn nix_cache_dir(identity: &DevIdentity) -> PathBuf {
    identity.home.join(".cache/nix")
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

#[cfg(test)]
#[path = "root_tests.rs"]
mod tests;
