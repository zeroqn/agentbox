use anyhow::{Context, Result, anyhow};
use std::path::{Path, PathBuf};

use crate::guest_init::command;

const PREFERRED_CONTAINERS_DISK: &str = "/dev/vdb";
const CONTAINERS_BTRFS_MOUNT_OPTIONS: &str = "user_subvol_rm_allowed";
pub(in crate::guest_init) const MOUNT_POINT: &str = "/home/dev/.local/share/containers";

pub(in crate::guest_init) fn find_disk(label: &str, disk_id: &str) -> Result<PathBuf> {
    crate::guest_init::components::disk::btrfs::find_labeled_disk_with_preferred(
        label,
        disk_id,
        &[PREFERRED_CONTAINERS_DISK],
    )
    .with_context(|| {
        format!("internal container storage btrfs disk not found (label={label} id={disk_id})")
    })
}

pub(in crate::guest_init) fn ensure_mounted(label: &str, disk_id: &str) -> Result<PathBuf> {
    ensure_mounted_with_ops(label, disk_id, &SystemContainersDiskOps)
}

fn ensure_mounted_with_ops(
    label: &str,
    disk_id: &str,
    ops: &impl ContainersDiskOps,
) -> Result<PathBuf> {
    let mount = Path::new(MOUNT_POINT);
    ops.create_dir_all(mount)?;
    let disk = ops.find_disk(label, disk_id)?;
    if ops.is_mounted(mount)? {
        return Ok(mount.to_path_buf());
    }
    match ops.mount_btrfs(&disk, mount) {
        Ok(()) => Ok(mount.to_path_buf()),
        Err(_err) if ops.is_mounted(mount).unwrap_or(false) => Ok(mount.to_path_buf()),
        Err(err) => Err(err).context("failed to mount internal container storage btrfs disk"),
    }
}

trait ContainersDiskOps {
    fn find_disk(&self, label: &str, disk_id: &str) -> Result<PathBuf>;
    fn create_dir_all(&self, path: &Path) -> Result<()>;
    fn is_mounted(&self, path: &Path) -> Result<bool>;
    fn mount_btrfs(&self, disk: &Path, mount: &Path) -> Result<()>;
}

#[derive(Debug, Clone, Copy)]
struct SystemContainersDiskOps;

impl ContainersDiskOps for SystemContainersDiskOps {
    fn find_disk(&self, label: &str, disk_id: &str) -> Result<PathBuf> {
        find_disk(label, disk_id)
    }

    fn create_dir_all(&self, path: &Path) -> Result<()> {
        crate::guest_init::fs::create_dir_all(path)
    }

    fn is_mounted(&self, path: &Path) -> Result<bool> {
        is_mounted(path)
    }

    fn mount_btrfs(&self, disk: &Path, mount: &Path) -> Result<()> {
        let args = mount_btrfs_args(disk, mount)?;
        command::run("mount", &args)
    }
}

fn mount_btrfs_args<'a>(disk: &'a Path, mount: &'a Path) -> Result<[&'a str; 6]> {
    Ok([
        "-t",
        "btrfs",
        "-o",
        CONTAINERS_BTRFS_MOUNT_OPTIONS,
        path_str(disk)?,
        path_str(mount)?,
    ])
}

fn is_mounted(path: &Path) -> Result<bool> {
    command::status_ok("findmnt", &["-rn", path_str(path)?])
}

fn path_str(path: &Path) -> Result<&str> {
    path.to_str()
        .ok_or_else(|| anyhow!("path is not valid UTF-8: {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[test]
    fn containers_btrfs_mount_args_enable_user_subvol_removal() {
        let args = mount_btrfs_args(Path::new("/dev/vdb"), Path::new(MOUNT_POINT)).unwrap();

        assert_eq!(
            args,
            [
                "-t",
                "btrfs",
                "-o",
                "user_subvol_rm_allowed",
                "/dev/vdb",
                MOUNT_POINT,
            ]
        );
    }

    #[test]
    fn ensure_mounted_reuses_existing_mount_without_remounting() {
        let ops = FakeContainersDiskOps::new(true);
        let mount = ensure_mounted_with_ops("loftd-containers", "disk-id", &ops).unwrap();

        assert_eq!(mount, PathBuf::from(MOUNT_POINT));
        assert_eq!(
            ops.operations.borrow().as_slice(),
            [
                "mkdir:/home/dev/.local/share/containers",
                "find:loftd-containers:disk-id",
                "findmnt:/home/dev/.local/share/containers",
            ]
        );
    }

    #[test]
    fn ensure_mounted_mounts_unmounted_containers_disk() {
        let ops = FakeContainersDiskOps::new(false);
        let mount = ensure_mounted_with_ops("loftd-containers", "disk-id", &ops).unwrap();

        assert_eq!(mount, PathBuf::from(MOUNT_POINT));
        assert_eq!(
            ops.operations.borrow().as_slice(),
            [
                "mkdir:/home/dev/.local/share/containers",
                "find:loftd-containers:disk-id",
                "findmnt:/home/dev/.local/share/containers",
                "mount:/dev/vdb:/home/dev/.local/share/containers",
            ]
        );
    }

    struct FakeContainersDiskOps {
        mounted: bool,
        operations: RefCell<Vec<String>>,
    }

    impl FakeContainersDiskOps {
        fn new(mounted: bool) -> Self {
            Self {
                mounted,
                operations: RefCell::new(Vec::new()),
            }
        }

        fn record(&self, operation: impl Into<String>) {
            self.operations.borrow_mut().push(operation.into());
        }
    }

    impl ContainersDiskOps for FakeContainersDiskOps {
        fn find_disk(&self, label: &str, disk_id: &str) -> Result<PathBuf> {
            self.record(format!("find:{label}:{disk_id}"));
            Ok(PathBuf::from("/dev/vdb"))
        }

        fn create_dir_all(&self, path: &Path) -> Result<()> {
            self.record(format!("mkdir:{}", path.display()));
            Ok(())
        }

        fn is_mounted(&self, path: &Path) -> Result<bool> {
            self.record(format!("findmnt:{}", path.display()));
            Ok(self.mounted)
        }

        fn mount_btrfs(&self, disk: &Path, mount: &Path) -> Result<()> {
            self.record(format!("mount:{}:{}", disk.display(), mount.display()));
            Ok(())
        }
    }
}
