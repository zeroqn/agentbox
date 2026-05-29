use anyhow::{Context, Result, anyhow};
use std::fs;
use std::os::unix::fs::PermissionsExt;
#[cfg(test)]
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::cli::MicrovmStoragePolicy;
use crate::runtime::microvm::image_cache::ImageCacheEntry;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StorageBackend {
    Auto,
    BtrfsSnapshot,
    Reflink,
    FuseOverlay,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MaterializedStorageBackend {
    BtrfsSnapshot,
    Reflink,
    FuseOverlay,
}

impl MaterializedStorageBackend {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::BtrfsSnapshot => "btrfs-snapshot",
            Self::Reflink => "reflink",
            Self::FuseOverlay => "fuse-overlay",
        }
    }
}

pub(crate) trait StorageCommands {
    fn btrfs_snapshot_available(&self) -> bool;
    fn reflink_copy_available(&self) -> bool;
    fn fuse_overlay_available(&self) -> bool;
    fn snapshot_btrfs_subvolume(&self, source: &Path, destination: &Path) -> Result<()>;
    fn delete_btrfs_subvolume(&self, subvolume: &Path) -> Result<()>;
    fn reflink_copy_tree(&self, source: &Path, destination: &Path) -> Result<()>;
    fn mount_fuse_overlay(
        &self,
        lowerdir: &Path,
        upperdir: &Path,
        workdir: &Path,
        merged: &Path,
    ) -> Result<()>;
    fn unmount_fuse_overlay(&self, merged: &Path) -> Result<()>;
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct HostStorageCommands;

impl StorageCommands for HostStorageCommands {
    fn btrfs_snapshot_available(&self) -> bool {
        command_available("buildah") && command_available("btrfs")
    }

    fn reflink_copy_available(&self) -> bool {
        command_available("cp")
    }

    fn fuse_overlay_available(&self) -> bool {
        command_available("fuse-overlayfs")
    }

    fn snapshot_btrfs_subvolume(&self, source: &Path, destination: &Path) -> Result<()> {
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create '{}'", parent.display()))?;
        }
        let output = buildah_unshare_btrfs_subvolume("snapshot", &[source, destination])
            .with_context(|| {
                format!(
                    "failed to run namespace-aware btrfs snapshot rootfs materialization from '{}' to '{}'",
                    source.display(),
                    destination.display()
                )
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!(
                "buildah unshare btrfs subvolume snapshot failed from '{}' to '{}': {}",
                source.display(),
                destination.display(),
                stderr.trim()
            );
        }
        Ok(())
    }

    fn delete_btrfs_subvolume(&self, subvolume: &Path) -> Result<()> {
        let output =
            buildah_unshare_btrfs_subvolume("delete", &[subvolume]).with_context(|| {
                format!(
                    "failed to run namespace-aware btrfs subvolume delete for '{}'",
                    subvolume.display()
                )
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!(
                "buildah unshare btrfs subvolume delete failed for '{}': {}",
                subvolume.display(),
                stderr.trim()
            );
        }
        Ok(())
    }

    fn reflink_copy_tree(&self, source: &Path, destination: &Path) -> Result<()> {
        fs::create_dir_all(destination)
            .with_context(|| format!("failed to create '{}'", destination.display()))?;
        let source_contents = source.join(".");
        let output = Command::new("cp")
            .arg("-a")
            .arg("--reflink=always")
            .arg(&source_contents)
            .arg(destination)
            .stdin(Stdio::null())
            .output()
            .map_err(|err| match err.kind() {
                std::io::ErrorKind::NotFound => {
                    anyhow!("cp is not installed or not available on PATH")
                }
                _ => err.into(),
            })
            .with_context(|| {
                format!(
                    "failed to run reflink rootfs materialization from '{}' to '{}'",
                    source.display(),
                    destination.display()
                )
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!(
                "cp -a --reflink=always failed from '{}' to '{}': {}",
                source.display(),
                destination.display(),
                stderr.trim()
            );
        }
        Ok(())
    }

    fn mount_fuse_overlay(
        &self,
        lowerdir: &Path,
        upperdir: &Path,
        workdir: &Path,
        merged: &Path,
    ) -> Result<()> {
        let overlay_opts = format!(
            "lowerdir={},upperdir={},workdir={}",
            lowerdir.display(),
            upperdir.display(),
            workdir.display()
        );
        let output = Command::new("fuse-overlayfs")
            .arg("-o")
            .arg(&overlay_opts)
            .arg(merged)
            .stdin(Stdio::null())
            .output()
            .map_err(|err| match err.kind() {
                std::io::ErrorKind::NotFound => {
                    anyhow!("fuse-overlayfs is not installed or not available on PATH")
                }
                _ => err.into(),
            })
            .with_context(|| {
                format!(
                    "failed to mount fuse-overlayfs with lowerdir='{}' upperdir='{}' workdir='{}'",
                    lowerdir.display(),
                    upperdir.display(),
                    workdir.display()
                )
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!(
                "fuse-overlayfs mount failed for '{}' (lower='{}', upper='{}', work='{}'): {}",
                merged.display(),
                lowerdir.display(),
                upperdir.display(),
                workdir.display(),
                stderr.trim()
            );
        }
        Ok(())
    }

    fn unmount_fuse_overlay(&self, merged: &Path) -> Result<()> {
        let mut attempted = Vec::new();
        for (command, args) in [
            ("fusermount3", &["-u"][..]),
            ("fusermount", &["-u"][..]),
            ("umount", &[][..]),
        ] {
            let output = Command::new(command)
                .args(args)
                .arg(merged)
                .stdin(Stdio::null())
                .output();
            match output {
                Ok(output) if output.status.success() => return Ok(()),
                Ok(output) => {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    attempted.push(format!("{command}: {}", stderr.trim()));
                }
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                    attempted.push(format!("{command}: not found"));
                }
                Err(err) => attempted.push(format!("{command}: {err}")),
            }
        }

        anyhow::bail!(
            "failed to unmount fuse-overlayfs task rootfs '{}': {}",
            merged.display(),
            attempted.join("; ")
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TaskRootfsHandle {
    pub(crate) root: PathBuf,
    task_dir: PathBuf,
    preserve_debug: bool,
    storage_backend: MaterializedStorageBackend,
    mounted_fuse_overlay: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CleanupResult {
    Removed,
    Preserved(PathBuf),
}

impl TaskRootfsHandle {
    pub(crate) fn task_dir(&self) -> &Path {
        &self.task_dir
    }

    pub(crate) fn storage_mode_label(&self) -> &'static str {
        self.storage_backend.label()
    }

    #[cfg(test)]
    pub(crate) fn requires_unmount(&self) -> bool {
        self.mounted_fuse_overlay
    }

    pub(crate) fn unmount_for_cleanup(&self, commands: &impl StorageCommands) -> Result<()> {
        if self.preserve_debug || !self.mounted_fuse_overlay {
            return Ok(());
        }
        commands.unmount_fuse_overlay(&self.root).with_context(|| {
            format!(
                "failed to unmount microvm task rootfs '{}'",
                self.root.display()
            )
        })
    }

    pub(crate) fn cleanup_state(self, commands: &impl StorageCommands) -> Result<CleanupResult> {
        if self.preserve_debug {
            return Ok(CleanupResult::Preserved(self.root));
        }
        if self.storage_backend == MaterializedStorageBackend::BtrfsSnapshot && self.root.exists() {
            commands
                .delete_btrfs_subvolume(&self.root)
                .with_context(|| btrfs_subvolume_delete_failure_hint(&self.root, &self.task_dir))
                .with_context(|| {
                    format!(
                        "failed to delete btrfs snapshot task rootfs '{}'",
                        self.root.display()
                    )
                })?;
        }
        if self.task_dir.exists() {
            remove_task_rootfs_tree(&self.task_dir).with_context(|| {
                format!(
                    "failed to clean up microvm task state dir '{}'",
                    self.task_dir.display()
                )
            })?;
        }
        Ok(CleanupResult::Removed)
    }

    pub(crate) fn preserve_debug_hint(&self) -> Option<String> {
        match self.storage_backend {
            MaterializedStorageBackend::BtrfsSnapshot => Some(format!(
                "btrfs snapshot task rootfs is preserved; delete it with `buildah unshare btrfs subvolume delete '{}'` before deleting '{}'. If deletion is denied, inspect the mount with `findmnt -T '{}' -o TARGET,SOURCE,FSTYPE,OPTIONS` and enable the btrfs `user_subvol_rm_allowed` mount option for rootless subvolume cleanup.",
                self.root.display(),
                self.task_dir.display(),
                self.root.display()
            )),
            MaterializedStorageBackend::FuseOverlay => Some(format!(
                "fuse-overlay task rootfs is still mounted; unmount it with `fusermount3 -u '{}'` before deleting '{}'",
                self.root.display(),
                self.task_dir.display()
            )),
            MaterializedStorageBackend::Reflink => None,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct StorageManager {
    state_root: PathBuf,
}

impl StorageManager {
    pub(crate) fn new(state_root: PathBuf) -> Self {
        Self { state_root }
    }

    pub(crate) fn select_backend(
        policy: MicrovmStoragePolicy,
        commands: &impl StorageCommands,
    ) -> Result<StorageBackend> {
        match policy {
            MicrovmStoragePolicy::Auto
                if commands.btrfs_snapshot_available() || commands.fuse_overlay_available() =>
            {
                Ok(StorageBackend::Auto)
            }
            MicrovmStoragePolicy::Auto => Err(anyhow!(
                "experimental microvm storage auto requires buildah+btrfs for snapshots or fuse-overlayfs on PATH"
            )),
            MicrovmStoragePolicy::BtrfsSnapshot if commands.btrfs_snapshot_available() => {
                Ok(StorageBackend::BtrfsSnapshot)
            }
            MicrovmStoragePolicy::BtrfsSnapshot => Err(anyhow!(
                "experimental microvm storage --storage btrfs-snapshot requires buildah and btrfs on PATH"
            )),
            MicrovmStoragePolicy::Reflink if commands.reflink_copy_available() => {
                Ok(StorageBackend::Reflink)
            }
            MicrovmStoragePolicy::Reflink => Err(anyhow!(
                "experimental microvm storage --storage reflink requires cp on PATH"
            )),
            MicrovmStoragePolicy::FuseOverlay if commands.fuse_overlay_available() => {
                Ok(StorageBackend::FuseOverlay)
            }
            MicrovmStoragePolicy::FuseOverlay => Err(anyhow!(
                "experimental microvm storage --storage fuse-overlay requires fuse-overlayfs on PATH"
            )),
        }
    }

    pub(crate) fn materialize(
        &self,
        entry: &ImageCacheEntry,
        backend: StorageBackend,
        task_id: &str,
        preserve_debug: bool,
        commands: &impl StorageCommands,
    ) -> Result<TaskRootfsHandle> {
        entry.ensure_agentbox_compatible()?;
        let task_dir = self.state_root.join("microvm-tasks").join(task_id);
        match backend {
            StorageBackend::Auto => self.materialize_auto(entry, &task_dir, preserve_debug, commands),
            StorageBackend::BtrfsSnapshot => self
                .materialize_btrfs_snapshot(entry, &task_dir, preserve_debug, commands)
                .with_context(|| {
                    format!(
                        "failed to materialize microvm task rootfs with btrfs snapshot from '{}'; refresh the microvm image cache if this cache entry was created before btrfs snapshot support",
                        entry.rootfs.display()
                    )
                }),
            StorageBackend::Reflink => self
                .materialize_reflink(entry, &task_dir, preserve_debug, commands)
                .with_context(|| {
                    format!(
                        "failed to materialize microvm task rootfs with required reflinks from '{}'",
                        entry.rootfs.display()
                    )
                }),
            StorageBackend::FuseOverlay => self
                .materialize_fuse_overlay(entry, &task_dir, preserve_debug, commands)
                .with_context(|| {
                    format!(
                        "failed to materialize microvm task rootfs with fuse-overlayfs from '{}'",
                        entry.rootfs.display()
                    )
                }),
        }
    }

    fn materialize_auto(
        &self,
        entry: &ImageCacheEntry,
        task_dir: &Path,
        preserve_debug: bool,
        commands: &impl StorageCommands,
    ) -> Result<TaskRootfsHandle> {
        let btrfs_result = if commands.btrfs_snapshot_available() {
            self.materialize_btrfs_snapshot(entry, task_dir, preserve_debug, commands)
        } else {
            Err(anyhow!(
                "btrfs is not available for snapshot materialization"
            ))
        };
        match btrfs_result {
            Ok(handle) => Ok(handle),
            Err(btrfs_err) => {
                reset_task_dir(task_dir, commands).with_context(|| {
                    format!(
                        "failed to clean partial btrfs-snapshot microvm task rootfs after: {btrfs_err:#}"
                    )
                })?;
                if !commands.fuse_overlay_available() {
                    return Err(anyhow!(
                        "auto microvm storage failed: btrfs-snapshot materialization failed ({btrfs_err:#}); fuse-overlayfs is not available on PATH"
                    ));
                }
                self.materialize_fuse_overlay(entry, task_dir, preserve_debug, commands)
                    .map_err(|fuse_err| {
                        anyhow!(
                            "auto microvm storage failed: btrfs-snapshot materialization failed ({btrfs_err:#}); fuse-overlay materialization failed ({fuse_err:#})"
                        )
                    })
            }
        }
    }

    fn materialize_btrfs_snapshot(
        &self,
        entry: &ImageCacheEntry,
        task_dir: &Path,
        preserve_debug: bool,
        commands: &impl StorageCommands,
    ) -> Result<TaskRootfsHandle> {
        reset_task_dir(task_dir, commands)?;
        let root = task_dir.join("rootfs-btrfs-snapshot");
        commands
            .snapshot_btrfs_subvolume(&entry.rootfs, &root)
            .map_err(|err| cleanup_after_materialization_failure(task_dir, commands, err))
            .with_context(|| {
                format!(
                    "failed to snapshot microvm task rootfs from '{}' to '{}'",
                    entry.rootfs.display(),
                    root.display()
                )
            })?;
        Ok(TaskRootfsHandle {
            root,
            task_dir: task_dir.to_path_buf(),
            preserve_debug,
            storage_backend: MaterializedStorageBackend::BtrfsSnapshot,
            mounted_fuse_overlay: false,
        })
    }

    fn materialize_reflink(
        &self,
        entry: &ImageCacheEntry,
        task_dir: &Path,
        preserve_debug: bool,
        commands: &impl StorageCommands,
    ) -> Result<TaskRootfsHandle> {
        reset_task_dir(task_dir, commands)?;
        let root = task_dir.join("rootfs-reflink");
        commands
            .reflink_copy_tree(&entry.rootfs, &root)
            .map_err(|err| cleanup_after_materialization_failure(task_dir, commands, err))
            .with_context(|| {
                format!(
                    "failed to reflink-copy microvm task rootfs from '{}' to '{}'",
                    entry.rootfs.display(),
                    root.display()
                )
            })?;
        Ok(TaskRootfsHandle {
            root,
            task_dir: task_dir.to_path_buf(),
            preserve_debug,
            storage_backend: MaterializedStorageBackend::Reflink,
            mounted_fuse_overlay: false,
        })
    }

    fn materialize_fuse_overlay(
        &self,
        entry: &ImageCacheEntry,
        task_dir: &Path,
        preserve_debug: bool,
        commands: &impl StorageCommands,
    ) -> Result<TaskRootfsHandle> {
        reset_task_dir(task_dir, commands)?;
        let root = task_dir.join("rootfs-fuse-overlay");
        let upper = task_dir.join("upper-fuse-overlay");
        let work = task_dir.join("work-fuse-overlay");
        fs::create_dir_all(&root)
            .with_context(|| format!("failed to create '{}'", root.display()))?;
        fs::create_dir_all(&upper)
            .with_context(|| format!("failed to create '{}'", upper.display()))?;
        fs::create_dir_all(&work)
            .with_context(|| format!("failed to create '{}'", work.display()))?;
        commands
            .mount_fuse_overlay(&entry.rootfs, &upper, &work, &root)
            .map_err(|err| cleanup_after_materialization_failure(task_dir, commands, err))
            .with_context(|| {
                format!(
                    "failed to mount microvm task rootfs from lower '{}' to merged '{}'",
                    entry.rootfs.display(),
                    root.display()
                )
            })?;
        Ok(TaskRootfsHandle {
            root,
            task_dir: task_dir.to_path_buf(),
            preserve_debug,
            storage_backend: MaterializedStorageBackend::FuseOverlay,
            mounted_fuse_overlay: true,
        })
    }
}

fn btrfs_subvolume_delete_failure_hint(rootfs: &Path, task_dir: &Path) -> String {
    format!(
        "btrfs-snapshot cleanup uses `btrfs subvolume delete`, which is fast but requires root/CAP_SYS_ADMIN unless the host btrfs mount allows user-owned subvolume removal. For rootless cleanup, enable the btrfs `user_subvol_rm_allowed` mount option on the filesystem containing this task rootfs. Inspect the relevant mount with `findmnt -T '{}' -o TARGET,SOURCE,FSTYPE,OPTIONS`; add `user_subvol_rm_allowed` to the OPTIONS for the matching btrfs entry in /etc/fstab; then remount with `sudo mount -o remount,user_subvol_rm_allowed <mountpoint>`. After remounting, retry manual cleanup with `buildah unshare btrfs subvolume delete '{}'` and then remove the task state dir '{}' if it remains",
        rootfs.display(),
        rootfs.display(),
        task_dir.display()
    )
}

fn cleanup_after_materialization_failure(
    task_dir: &Path,
    commands: &impl StorageCommands,
    err: anyhow::Error,
) -> anyhow::Error {
    match reset_task_dir(task_dir, commands) {
        Ok(()) => err,
        Err(cleanup_err) => cleanup_err.context(format!(
            "failed to clean partial microvm task rootfs after materialization error: {err:#}"
        )),
    }
}

fn buildah_unshare_btrfs_subvolume(action: &str, paths: &[&Path]) -> Result<std::process::Output> {
    Command::new("buildah")
        .arg("unshare")
        .arg("btrfs")
        .arg("subvolume")
        .arg(action)
        .args(paths)
        .stdin(Stdio::null())
        .output()
        .map_err(|err| match err.kind() {
            std::io::ErrorKind::NotFound => anyhow!(
                "buildah is not installed or not available on PATH; btrfs-snapshot storage requires buildah unshare"
            ),
            _ => err.into(),
        })
}

fn command_available(command: &str) -> bool {
    std::process::Command::new(command)
        .arg("--help")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok()
}

fn reset_task_dir(task_dir: &Path, commands: &impl StorageCommands) -> Result<()> {
    if task_dir.exists() {
        let btrfs_root = task_dir.join("rootfs-btrfs-snapshot");
        if btrfs_root.exists() {
            let _ = commands.delete_btrfs_subvolume(&btrfs_root);
        }
        remove_task_rootfs_tree(task_dir).with_context(|| {
            format!(
                "failed to reset stale microvm task state dir '{}'",
                task_dir.display()
            )
        })?;
    }
    Ok(())
}

fn remove_task_rootfs_tree(root: &Path) -> Result<()> {
    make_directories_owner_writable(root)?;
    fs::remove_dir_all(root)?;
    Ok(())
}

fn make_directories_owner_writable(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("failed to stat '{}'", path.display()))?;
    if !metadata.is_dir() {
        return Ok(());
    }

    let mode = metadata.permissions().mode();
    if mode & 0o200 == 0 {
        fs::set_permissions(path, fs::Permissions::from_mode(mode | 0o700)).with_context(|| {
            format!(
                "failed to make microvm task rootfs directory writable '{}'",
                path.display()
            )
        })?;
    }

    for entry in
        fs::read_dir(path).with_context(|| format!("failed to read '{}'", path.display()))?
    {
        make_directories_owner_writable(&entry?.path())?;
    }
    Ok(())
}

#[cfg(test)]
pub(crate) fn copy_rootfs_tree(source: &Path, destination: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(source)
        .with_context(|| format!("failed to stat '{}'", source.display()))?;
    if metadata.file_type().is_symlink() {
        copy_symlink(source, destination)
    } else if metadata.is_dir() {
        copy_dir(source, destination, &metadata)
    } else if metadata.is_file() {
        copy_file(source, destination, &metadata)
    } else {
        anyhow::bail!("unsupported rootfs entry type '{}'", source.display())
    }
}

#[cfg(test)]
fn copy_dir(source: &Path, destination: &Path, metadata: &fs::Metadata) -> Result<()> {
    fs::create_dir_all(destination)
        .with_context(|| format!("failed to create '{}'", destination.display()))?;
    for entry in
        fs::read_dir(source).with_context(|| format!("failed to read '{}'", source.display()))?
    {
        let entry = entry?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        copy_rootfs_tree(&source_path, &destination_path)?;
    }
    fs::set_permissions(
        destination,
        fs::Permissions::from_mode(metadata.permissions().mode()),
    )
    .with_context(|| format!("failed to preserve mode on '{}'", destination.display()))?;
    Ok(())
}

#[cfg(test)]
fn copy_file(source: &Path, destination: &Path, metadata: &fs::Metadata) -> Result<()> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create '{}'", parent.display()))?;
    }
    fs::copy(source, destination).with_context(|| {
        format!(
            "failed to copy '{}' to '{}'",
            source.display(),
            destination.display()
        )
    })?;
    fs::set_permissions(
        destination,
        fs::Permissions::from_mode(metadata.permissions().mode()),
    )
    .with_context(|| format!("failed to preserve mode on '{}'", destination.display()))?;
    Ok(())
}

#[cfg(test)]
fn copy_symlink(source: &Path, destination: &Path) -> Result<()> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create '{}'", parent.display()))?;
    }
    let target = fs::read_link(source)
        .with_context(|| format!("failed to read symlink '{}'", source.display()))?;
    symlink(&target, destination).with_context(|| {
        format!(
            "failed to copy symlink '{}' to '{}'",
            source.display(),
            destination.display()
        )
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::microvm::image_cache::{
        BuildahRunner, ImageCache, ImageDigest, ImageReference,
    };
    use std::cell::RefCell;
    use std::collections::VecDeque;

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum StorageCall {
        BtrfsSnapshot {
            source: PathBuf,
            destination: PathBuf,
        },
        DeleteBtrfsSubvolume(PathBuf),
        ReflinkCopy {
            source: PathBuf,
            destination: PathBuf,
        },
        MountFuseOverlay {
            lowerdir: PathBuf,
            upperdir: PathBuf,
            workdir: PathBuf,
            merged: PathBuf,
        },
        UnmountFuseOverlay(PathBuf),
    }

    #[derive(Debug)]
    struct FakeStorageCommands {
        btrfs_available: bool,
        reflink_available: bool,
        fuse_available: bool,
        btrfs_errors: RefCell<VecDeque<&'static str>>,
        btrfs_delete_errors: RefCell<VecDeque<&'static str>>,
        reflink_errors: RefCell<VecDeque<&'static str>>,
        fuse_errors: RefCell<VecDeque<&'static str>>,
        unmount_errors: RefCell<VecDeque<&'static str>>,
        calls: RefCell<Vec<StorageCall>>,
    }

    impl FakeStorageCommands {
        fn available() -> Self {
            Self {
                btrfs_available: true,
                reflink_available: true,
                fuse_available: true,
                btrfs_errors: RefCell::new(VecDeque::new()),
                btrfs_delete_errors: RefCell::new(VecDeque::new()),
                reflink_errors: RefCell::new(VecDeque::new()),
                fuse_errors: RefCell::new(VecDeque::new()),
                unmount_errors: RefCell::new(VecDeque::new()),
                calls: RefCell::new(Vec::new()),
            }
        }

        fn missing() -> Self {
            Self {
                btrfs_available: false,
                reflink_available: false,
                fuse_available: false,
                btrfs_errors: RefCell::new(VecDeque::new()),
                btrfs_delete_errors: RefCell::new(VecDeque::new()),
                reflink_errors: RefCell::new(VecDeque::new()),
                fuse_errors: RefCell::new(VecDeque::new()),
                unmount_errors: RefCell::new(VecDeque::new()),
                calls: RefCell::new(Vec::new()),
            }
        }

        fn without_btrfs() -> Self {
            Self {
                btrfs_available: false,
                ..Self::available()
            }
        }

        fn without_fuse() -> Self {
            Self {
                fuse_available: false,
                ..Self::available()
            }
        }

        fn without_reflink() -> Self {
            Self {
                reflink_available: false,
                ..Self::available()
            }
        }

        fn fail_btrfs(self, message: &'static str) -> Self {
            self.btrfs_errors.borrow_mut().push_back(message);
            self
        }

        fn fail_btrfs_delete(self, message: &'static str) -> Self {
            self.btrfs_delete_errors.borrow_mut().push_back(message);
            self
        }

        fn fail_reflink(self, message: &'static str) -> Self {
            self.reflink_errors.borrow_mut().push_back(message);
            self
        }

        fn fail_fuse(self, message: &'static str) -> Self {
            self.fuse_errors.borrow_mut().push_back(message);
            self
        }

        fn fail_unmount(self, message: &'static str) -> Self {
            self.unmount_errors.borrow_mut().push_back(message);
            self
        }

        fn calls(&self) -> Vec<StorageCall> {
            self.calls.borrow().clone()
        }
    }

    impl StorageCommands for FakeStorageCommands {
        fn btrfs_snapshot_available(&self) -> bool {
            self.btrfs_available
        }

        fn reflink_copy_available(&self) -> bool {
            self.reflink_available
        }

        fn fuse_overlay_available(&self) -> bool {
            self.fuse_available
        }

        fn snapshot_btrfs_subvolume(&self, source: &Path, destination: &Path) -> Result<()> {
            self.calls.borrow_mut().push(StorageCall::BtrfsSnapshot {
                source: source.to_path_buf(),
                destination: destination.to_path_buf(),
            });
            if let Some(message) = self.btrfs_errors.borrow_mut().pop_front() {
                anyhow::bail!(message);
            }
            copy_rootfs_tree(source, destination)
        }

        fn delete_btrfs_subvolume(&self, subvolume: &Path) -> Result<()> {
            self.calls
                .borrow_mut()
                .push(StorageCall::DeleteBtrfsSubvolume(subvolume.to_path_buf()));
            if let Some(message) = self.btrfs_delete_errors.borrow_mut().pop_front() {
                anyhow::bail!(message);
            }
            if subvolume.exists() {
                fs::remove_dir_all(subvolume).with_context(|| {
                    format!(
                        "failed to remove fake btrfs subvolume '{}'",
                        subvolume.display()
                    )
                })?;
            }
            Ok(())
        }

        fn reflink_copy_tree(&self, source: &Path, destination: &Path) -> Result<()> {
            self.calls.borrow_mut().push(StorageCall::ReflinkCopy {
                source: source.to_path_buf(),
                destination: destination.to_path_buf(),
            });
            if let Some(message) = self.reflink_errors.borrow_mut().pop_front() {
                anyhow::bail!(message);
            }
            copy_rootfs_tree(source, destination)
        }

        fn mount_fuse_overlay(
            &self,
            lowerdir: &Path,
            upperdir: &Path,
            workdir: &Path,
            merged: &Path,
        ) -> Result<()> {
            self.calls.borrow_mut().push(StorageCall::MountFuseOverlay {
                lowerdir: lowerdir.to_path_buf(),
                upperdir: upperdir.to_path_buf(),
                workdir: workdir.to_path_buf(),
                merged: merged.to_path_buf(),
            });
            if let Some(message) = self.fuse_errors.borrow_mut().pop_front() {
                anyhow::bail!(message);
            }
            copy_rootfs_tree(lowerdir, merged)
        }

        fn unmount_fuse_overlay(&self, merged: &Path) -> Result<()> {
            self.calls
                .borrow_mut()
                .push(StorageCall::UnmountFuseOverlay(merged.to_path_buf()));
            if let Some(message) = self.unmount_errors.borrow_mut().pop_front() {
                anyhow::bail!(message);
            }
            Ok(())
        }
    }

    fn cached_entry(temp: &tempfile::TempDir) -> ImageCacheEntry {
        let cache = ImageCache::new(temp.path().join("images"));
        let digest = ImageDigest::parse("sha256:abc123").expect("digest should parse");
        let entry_dir = cache.entry_path(&digest);
        fs::create_dir_all(entry_dir.join("rootfs").join("etc"))
            .expect("cache rootfs should be created");
        fs::write(
            entry_dir.join("rootfs").join("etc").join("agentbox"),
            "cached",
        )
        .expect("cache content should be written");
        fs::write(entry_dir.join("agentbox-compatible"), "agentbox\n")
            .expect("compatibility marker should be written");
        cache
            .ensure(
                ImageReference::from_cli(Some("ghcr.io/example/agentbox@sha256:abc123")),
                &NoBuildah,
            )
            .expect("cache hit should resolve")
    }

    fn set_mode(path: &Path, mode: u32) {
        fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap_or_else(|err| {
            panic!("failed to set mode {mode:o} on '{}': {err}", path.display())
        });
    }

    struct NoBuildah;

    impl BuildahRunner for NoBuildah {
        fn ingest(&self, _reference: &ImageReference, _cache_root: &Path) -> Result<ImageDigest> {
            anyhow::bail!("buildah should not be called")
        }
    }

    #[test]
    fn auto_storage_selects_auto_when_any_cow_backend_is_available() {
        assert_eq!(
            StorageManager::select_backend(
                MicrovmStoragePolicy::Auto,
                &FakeStorageCommands::available()
            )
            .expect("auto should be available"),
            StorageBackend::Auto
        );
        assert_eq!(
            StorageManager::select_backend(
                MicrovmStoragePolicy::Auto,
                &FakeStorageCommands::without_btrfs(),
            )
            .expect("auto should use fuse-overlay when btrfs is missing"),
            StorageBackend::Auto
        );
        let no_fallback = FakeStorageCommands {
            btrfs_available: false,
            fuse_available: false,
            ..FakeStorageCommands::available()
        };
        assert!(StorageManager::select_backend(MicrovmStoragePolicy::Auto, &no_fallback).is_err());
    }

    #[test]
    fn explicit_storage_policy_reports_missing_helper() {
        let btrfs_error = StorageManager::select_backend(
            MicrovmStoragePolicy::BtrfsSnapshot,
            &FakeStorageCommands::without_btrfs(),
        )
        .expect_err("explicit btrfs-snapshot should fail when btrfs is unavailable");
        assert!(format!("{btrfs_error:#}").contains("btrfs-snapshot"));

        let reflink_error = StorageManager::select_backend(
            MicrovmStoragePolicy::Reflink,
            &FakeStorageCommands::without_reflink(),
        )
        .expect_err("explicit reflink should fail when cp is unavailable");
        assert!(format!("{reflink_error:#}").contains("reflink"));

        let fuse_error = StorageManager::select_backend(
            MicrovmStoragePolicy::FuseOverlay,
            &FakeStorageCommands::without_fuse(),
        )
        .expect_err("explicit fuse-overlay should fail when unavailable");
        assert!(format!("{fuse_error:#}").contains("fuse-overlayfs"));

        let missing_error = StorageManager::select_backend(
            MicrovmStoragePolicy::Auto,
            &FakeStorageCommands::missing(),
        )
        .expect_err("auto should fail when no CoW backend is available");
        assert!(format!("{missing_error:#}").contains("buildah+btrfs"));
    }

    #[test]
    fn btrfs_snapshot_materialization_requires_btrfs_snapshot_command() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let entry = cached_entry(&temp);
        let commands = FakeStorageCommands::available();
        let manager = StorageManager::new(temp.path().join("workspace-state"));

        let handle = manager
            .materialize(
                &entry,
                StorageBackend::BtrfsSnapshot,
                "task-btrfs",
                false,
                &commands,
            )
            .expect("task rootfs should materialize");

        assert_eq!(handle.root.file_name().unwrap(), "rootfs-btrfs-snapshot");
        assert_eq!(handle.storage_mode_label(), "btrfs-snapshot");
        assert!(!handle.requires_unmount());
        assert_eq!(
            fs::read_to_string(handle.root.join("etc").join("agentbox"))
                .expect("cached content should be visible"),
            "cached"
        );
        assert_eq!(
            commands.calls(),
            vec![StorageCall::BtrfsSnapshot {
                source: entry.rootfs,
                destination: handle.root,
            }]
        );
    }

    #[test]
    fn explicit_btrfs_snapshot_failure_does_not_fall_back_to_fuse_overlay() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let entry = cached_entry(&temp);
        let commands = FakeStorageCommands::available().fail_btrfs("not a subvolume");
        let manager = StorageManager::new(temp.path().join("workspace-state"));

        let error = manager
            .materialize(
                &entry,
                StorageBackend::BtrfsSnapshot,
                "task-btrfs",
                false,
                &commands,
            )
            .expect_err("required btrfs snapshot should fail");

        let message = format!("{error:#}");
        assert!(message.contains("btrfs snapshot"));
        assert!(message.contains("not a subvolume"));
        assert!(
            !temp
                .path()
                .join("workspace-state/microvm-tasks/task-btrfs")
                .exists()
        );
        assert!(matches!(
            commands.calls().as_slice(),
            [StorageCall::BtrfsSnapshot { .. }]
        ));
    }

    #[test]
    fn reflink_materialization_requires_reflink_copy_command() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let entry = cached_entry(&temp);
        let commands = FakeStorageCommands::available();
        let manager = StorageManager::new(temp.path().join("workspace-state"));

        let handle = manager
            .materialize(&entry, StorageBackend::Reflink, "task-1", false, &commands)
            .expect("task rootfs should materialize");

        assert_eq!(handle.root.file_name().unwrap(), "rootfs-reflink");
        assert!(!handle.requires_unmount());
        assert_eq!(
            fs::read_to_string(handle.root.join("etc").join("agentbox"))
                .expect("cached content should be visible"),
            "cached"
        );
        assert_eq!(
            commands.calls(),
            vec![StorageCall::ReflinkCopy {
                source: entry.rootfs,
                destination: handle.root,
            }]
        );
    }

    #[test]
    fn explicit_reflink_failure_does_not_fall_back_to_fuse_overlay() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let entry = cached_entry(&temp);
        let commands = FakeStorageCommands::available().fail_reflink("reflink unsupported");
        let manager = StorageManager::new(temp.path().join("workspace-state"));

        let error = manager
            .materialize(&entry, StorageBackend::Reflink, "task-1", false, &commands)
            .expect_err("required reflink should fail");

        let message = format!("{error:#}");
        assert!(message.contains("required reflinks"));
        assert!(message.contains("reflink unsupported"));
        assert!(
            !temp
                .path()
                .join("workspace-state/microvm-tasks/task-1")
                .exists()
        );
        assert!(matches!(
            commands.calls().as_slice(),
            [StorageCall::ReflinkCopy { .. }]
        ));
    }

    #[test]
    fn auto_btrfs_snapshot_failure_falls_back_to_real_fuse_overlay_backend() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let entry = cached_entry(&temp);
        let commands = FakeStorageCommands::available().fail_btrfs("not a subvolume");
        let manager = StorageManager::new(temp.path().join("workspace-state"));

        let handle = manager
            .materialize(&entry, StorageBackend::Auto, "task-auto", false, &commands)
            .expect("auto should fall back to fuse-overlay");

        let task_dir = handle.task_dir().to_path_buf();
        assert_eq!(handle.root, task_dir.join("rootfs-fuse-overlay"));
        assert!(handle.requires_unmount());
        assert!(task_dir.join("upper-fuse-overlay").is_dir());
        assert!(task_dir.join("work-fuse-overlay").is_dir());
        assert!(!task_dir.join("rootfs-btrfs-snapshot").exists());
        assert!(!task_dir.join("rootfs-reflink").exists());
        assert_eq!(
            fs::read_to_string(handle.root.join("etc/agentbox"))
                .expect("fuse overlay view should expose cached content"),
            "cached"
        );
        assert!(matches!(
            commands.calls().as_slice(),
            [
                StorageCall::BtrfsSnapshot { .. },
                StorageCall::MountFuseOverlay { .. }
            ]
        ));
    }

    #[test]
    fn auto_reports_both_btrfs_snapshot_and_fuse_overlay_failures() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let entry = cached_entry(&temp);
        let commands = FakeStorageCommands::available()
            .fail_btrfs("not a subvolume")
            .fail_fuse("fuse denied");
        let manager = StorageManager::new(temp.path().join("workspace-state"));

        let error = manager
            .materialize(&entry, StorageBackend::Auto, "task-auto", false, &commands)
            .expect_err("auto should report both failures");

        let message = format!("{error:#}");
        assert!(message.contains("auto microvm storage failed"));
        assert!(message.contains("not a subvolume"));
        assert!(message.contains("fuse denied"));
        assert!(
            !temp
                .path()
                .join("workspace-state/microvm-tasks/task-auto")
                .exists()
        );
    }

    #[test]
    fn fuse_overlay_materialization_mounts_expected_overlay_dirs() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let entry = cached_entry(&temp);
        let commands = FakeStorageCommands::available();
        let manager = StorageManager::new(temp.path().join("workspace-state"));

        let handle = manager
            .materialize(
                &entry,
                StorageBackend::FuseOverlay,
                "task-fuse",
                false,
                &commands,
            )
            .expect("fuse overlay rootfs should materialize");

        let task_dir = handle.task_dir().to_path_buf();
        assert_eq!(handle.root, task_dir.join("rootfs-fuse-overlay"));
        assert!(handle.requires_unmount());
        assert_eq!(
            commands.calls(),
            vec![StorageCall::MountFuseOverlay {
                lowerdir: entry.rootfs,
                upperdir: task_dir.join("upper-fuse-overlay"),
                workdir: task_dir.join("work-fuse-overlay"),
                merged: task_dir.join("rootfs-fuse-overlay"),
            }]
        );
    }

    #[test]
    fn fuse_overlay_cleanup_unmounts_then_removes_task_state_dir() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let entry = cached_entry(&temp);
        let commands = FakeStorageCommands::available();
        let manager = StorageManager::new(temp.path().join("workspace-state"));
        let handle = manager
            .materialize(
                &entry,
                StorageBackend::FuseOverlay,
                "task-clean",
                false,
                &commands,
            )
            .expect("fuse overlay rootfs should materialize");
        fs::write(handle.task_dir().join("launch.conf"), "config")
            .expect("launch config should be written");
        let root = handle.root.clone();
        let task_dir = handle.task_dir().to_path_buf();

        handle
            .unmount_for_cleanup(&commands)
            .expect("unmount should succeed");
        assert_eq!(
            handle
                .cleanup_state(&commands)
                .expect("cleanup should succeed"),
            CleanupResult::Removed
        );

        assert!(
            commands
                .calls()
                .contains(&StorageCall::UnmountFuseOverlay(root))
        );
        assert!(!task_dir.exists());
    }

    #[test]
    fn btrfs_snapshot_cleanup_deletes_subvolume_then_removes_task_state_dir() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let entry = cached_entry(&temp);
        let commands = FakeStorageCommands::available();
        let manager = StorageManager::new(temp.path().join("workspace-state"));
        let handle = manager
            .materialize(
                &entry,
                StorageBackend::BtrfsSnapshot,
                "task-clean-btrfs",
                false,
                &commands,
            )
            .expect("btrfs snapshot rootfs should materialize");
        fs::write(handle.task_dir().join("launch.conf"), "config")
            .expect("launch config should be written");
        let root = handle.root.clone();
        let task_dir = handle.task_dir().to_path_buf();

        handle
            .unmount_for_cleanup(&commands)
            .expect("btrfs cleanup has no unmount");
        assert_eq!(
            handle
                .cleanup_state(&commands)
                .expect("cleanup should succeed"),
            CleanupResult::Removed
        );

        assert!(
            commands
                .calls()
                .contains(&StorageCall::DeleteBtrfsSubvolume(root))
        );
        assert!(!task_dir.exists());
    }

    #[test]
    fn btrfs_snapshot_cleanup_delete_failure_preserves_task_state_for_manual_recovery() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let entry = cached_entry(&temp);
        let commands =
            FakeStorageCommands::available().fail_btrfs_delete("operation not permitted");
        let manager = StorageManager::new(temp.path().join("workspace-state"));
        let handle = manager
            .materialize(
                &entry,
                StorageBackend::BtrfsSnapshot,
                "task-clean-btrfs-fail",
                false,
                &commands,
            )
            .expect("btrfs snapshot rootfs should materialize");
        fs::write(handle.task_dir().join("launch.conf"), "config")
            .expect("launch config should be written");
        let root = handle.root.clone();
        let task_dir = handle.task_dir().to_path_buf();

        let cleanup_error = handle
            .cleanup_state(&commands)
            .expect_err("failed btrfs delete should stop cleanup");

        let cleanup_message = format!("{cleanup_error:#}");
        assert!(cleanup_message.contains("failed to delete btrfs snapshot"));
        assert!(cleanup_message.contains("operation not permitted"));
        assert!(cleanup_message.contains("user_subvol_rm_allowed"));
        assert!(cleanup_message.contains("findmnt -T"));
        assert!(cleanup_message.contains("/etc/fstab"));
        assert!(cleanup_message.contains("sudo mount -o remount,user_subvol_rm_allowed"));
        assert!(
            commands
                .calls()
                .contains(&StorageCall::DeleteBtrfsSubvolume(root))
        );
        assert!(task_dir.join("launch.conf").exists());
    }

    #[test]
    fn fuse_overlay_unmount_failure_prevents_task_dir_removal() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let entry = cached_entry(&temp);
        let commands = FakeStorageCommands::available().fail_unmount("busy");
        let manager = StorageManager::new(temp.path().join("workspace-state"));
        let handle = manager
            .materialize(
                &entry,
                StorageBackend::FuseOverlay,
                "task-busy",
                false,
                &commands,
            )
            .expect("fuse overlay rootfs should materialize");
        let task_dir = handle.task_dir().to_path_buf();

        let error = handle
            .unmount_for_cleanup(&commands)
            .expect_err("unmount should fail");

        assert!(format!("{error:#}").contains("busy"));
        assert!(task_dir.exists());
    }

    #[test]
    fn preserve_debug_keeps_mounted_fuse_overlay_and_reports_cleanup_hint() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let entry = cached_entry(&temp);
        let commands = FakeStorageCommands::available();
        let manager = StorageManager::new(temp.path().join("workspace-state"));
        let handle = manager
            .materialize(
                &entry,
                StorageBackend::FuseOverlay,
                "task-debug",
                true,
                &commands,
            )
            .expect("fuse overlay rootfs should materialize");
        let root = handle.root.clone();
        let task_dir = handle.task_dir().to_path_buf();
        let hint = handle.preserve_debug_hint().expect("fuse overlay hint");

        handle
            .unmount_for_cleanup(&commands)
            .expect("preserve-debug should skip unmount");
        assert_eq!(
            handle
                .cleanup_state(&commands)
                .expect("cleanup should preserve"),
            CleanupResult::Preserved(root.clone())
        );

        assert!(root.exists());
        assert!(task_dir.exists());
        assert!(hint.contains("fusermount3 -u"));
        assert!(
            !commands
                .calls()
                .iter()
                .any(|call| matches!(call, StorageCall::UnmountFuseOverlay(_)))
        );
    }

    #[test]
    fn preserve_debug_keeps_btrfs_snapshot_and_reports_cleanup_hint() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let entry = cached_entry(&temp);
        let commands = FakeStorageCommands::available();
        let manager = StorageManager::new(temp.path().join("workspace-state"));
        let handle = manager
            .materialize(
                &entry,
                StorageBackend::BtrfsSnapshot,
                "task-debug-btrfs",
                true,
                &commands,
            )
            .expect("btrfs snapshot rootfs should materialize");
        let root = handle.root.clone();
        let task_dir = handle.task_dir().to_path_buf();
        let hint = handle.preserve_debug_hint().expect("btrfs hint");

        handle
            .unmount_for_cleanup(&commands)
            .expect("preserve-debug should skip unmount");
        assert_eq!(
            handle
                .cleanup_state(&commands)
                .expect("cleanup should preserve"),
            CleanupResult::Preserved(root.clone())
        );

        assert!(root.exists());
        assert!(task_dir.exists());
        assert!(hint.contains("buildah unshare btrfs subvolume delete"));
        assert!(hint.contains("user_subvol_rm_allowed"));
        assert!(hint.contains("findmnt -T"));
        assert!(
            !commands
                .calls()
                .iter()
                .any(|call| matches!(call, StorageCall::DeleteBtrfsSubvolume(_)))
        );
    }

    #[test]
    fn cleanup_removes_reflink_task_rootfs_with_read_only_directories() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let entry = cached_entry(&temp);
        let read_only_dir = entry.rootfs.join("nix/store/hash/bin");
        fs::create_dir_all(&read_only_dir).expect("read-only dir should be created");
        fs::write(read_only_dir.join("tool"), "#!/bin/sh\n").expect("tool should be written");
        set_mode(&read_only_dir, 0o555);
        let commands = FakeStorageCommands::available();
        let manager = StorageManager::new(temp.path().join("workspace-state"));

        let handle = manager
            .materialize(
                &entry,
                StorageBackend::Reflink,
                "readonly-cleanup",
                false,
                &commands,
            )
            .expect("task rootfs should materialize");
        set_mode(&read_only_dir, 0o755);
        fs::write(handle.task_dir().join("launch.conf"), "config")
            .expect("launch config should be written");
        let copied_read_only_dir = handle.root.join("nix/store/hash/bin");
        assert_eq!(
            fs::metadata(&copied_read_only_dir)
                .expect("copied dir metadata should be readable")
                .permissions()
                .mode()
                & 0o777,
            0o555
        );
        let root = handle.root.clone();
        let task_dir = handle.task_dir().to_path_buf();

        handle
            .unmount_for_cleanup(&commands)
            .expect("reflink cleanup has no unmount");
        assert_eq!(
            handle
                .cleanup_state(&commands)
                .expect("cleanup should remove read-only dirs"),
            CleanupResult::Removed
        );
        assert!(!root.exists());
        assert!(!task_dir.exists());
    }

    #[test]
    fn materialize_resets_stale_read_only_task_state_dir() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let entry = cached_entry(&temp);
        let read_only_dir = entry.rootfs.join("nix/store/hash/bin");
        fs::create_dir_all(&read_only_dir).expect("read-only dir should be created");
        fs::write(read_only_dir.join("tool"), "#!/bin/sh\n").expect("tool should be written");
        set_mode(&read_only_dir, 0o555);
        let commands = FakeStorageCommands::available();
        let manager = StorageManager::new(temp.path().join("workspace-state"));

        let stale = manager
            .materialize(
                &entry,
                StorageBackend::Reflink,
                "readonly-reset",
                true,
                &commands,
            )
            .expect("stale task rootfs should materialize");
        fs::write(stale.task_dir().join("launch.conf"), "stale")
            .expect("stale launch config should be written");
        assert!(stale.root.exists());
        set_mode(&read_only_dir, 0o755);

        let replacement = manager
            .materialize(
                &entry,
                StorageBackend::Reflink,
                "readonly-reset",
                false,
                &commands,
            )
            .expect("stale read-only task state should be reset");
        let replacement_root = replacement.root.clone();

        assert!(replacement_root.exists());
        assert!(!replacement.task_dir().join("launch.conf").exists());
        assert_eq!(
            replacement
                .cleanup_state(&commands)
                .expect("replacement cleanup should succeed"),
            CleanupResult::Removed
        );
    }

    #[test]
    fn rootfs_tree_copy_helper_preserves_executable_modes_and_symlinks_for_image_ingestion() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let source = temp.path().join("source");
        let destination = temp.path().join("destination");
        let bin_dir = source.join("usr/bin");
        fs::create_dir_all(&bin_dir).expect("bin dir should be created");
        let tool = bin_dir.join("tool");
        fs::write(&tool, "#!/bin/sh\n").expect("tool should be written");
        fs::set_permissions(&tool, fs::Permissions::from_mode(0o755))
            .expect("tool mode should be set");
        std::os::unix::fs::symlink("tool", bin_dir.join("tool-link"))
            .expect("symlink should be created");

        copy_rootfs_tree(&source, &destination).expect("rootfs helper should copy");

        assert_eq!(
            fs::metadata(destination.join("usr/bin/tool"))
                .expect("tool metadata should be readable")
                .permissions()
                .mode()
                & 0o777,
            0o755
        );
        assert_eq!(
            fs::read_link(destination.join("usr/bin/tool-link"))
                .expect("symlink should be preserved"),
            PathBuf::from("tool")
        );
    }
}
