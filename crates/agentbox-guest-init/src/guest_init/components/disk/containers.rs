use anyhow::{Context, Result, anyhow};
use std::path::{Path, PathBuf};

use crate::guest_init::command;

const PREFERRED_CONTAINERS_DISK: &str = "/dev/vdb";
pub(in crate::guest_init) const MOUNT_POINT: &str = "/home/dev/.local/share/containers";

pub(in crate::guest_init) fn find_disk(label: &str, disk_id: &str) -> Result<PathBuf> {
    crate::guest_init::components::disk::btrfs::find_labeled_disk_with_preferred(
        label,
        disk_id,
        &[PREFERRED_CONTAINERS_DISK],
    )
    .with_context(|| {
        format!("libkrun container storage btrfs disk not found (label={label} id={disk_id})")
    })
}

pub(in crate::guest_init) fn ensure_mounted(label: &str, disk_id: &str) -> Result<PathBuf> {
    let mount = Path::new(MOUNT_POINT);
    crate::guest_init::fs::create_dir_all(mount)?;
    let disk = find_disk(label, disk_id)?;
    if is_mounted(mount)? {
        return Ok(mount.to_path_buf());
    }
    match command::run(
        "mount",
        &["-t", "btrfs", path_str(&disk)?, path_str(mount)?],
    ) {
        Ok(()) => Ok(mount.to_path_buf()),
        Err(_err) if is_mounted(mount).unwrap_or(false) => Ok(mount.to_path_buf()),
        Err(err) => Err(err).context("failed to mount libkrun container storage btrfs disk"),
    }
}

fn is_mounted(path: &Path) -> Result<bool> {
    command::status_ok("findmnt", &["-rn", path_str(path)?])
}

fn path_str(path: &Path) -> Result<&str> {
    path.to_str()
        .ok_or_else(|| anyhow!("path is not valid UTF-8: {}", path.display()))
}
