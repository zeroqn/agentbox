use anyhow::{Context, Result, anyhow};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::runtime::image_source::{self, BuildahCommands};
use crate::runtime::launch_plan::ImageSelection;
use crate::task_rootfs::TaskRootfsBackend;

const TASKS_DIR: &str = "tasks";
const BTRFS_ROOTFS_DIR: &str = "rootfs-btrfs-snapshot";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TaskRootfsHandle {
    task_id: String,
    task_dir: PathBuf,
    rootfs_path: PathBuf,
    backend: TaskRootfsBackend,
    selected_image_reference: String,
    image_digest: Option<String>,
    preserve_debug: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CleanupResult {
    Removed,
    Preserved(PathBuf),
}

pub(crate) struct TaskRootfsLease<C: BtrfsRootfsCommands> {
    handle: Option<TaskRootfsHandle>,
    commands: C,
}

impl<C: BtrfsRootfsCommands> TaskRootfsLease<C> {
    pub(crate) fn new(handle: TaskRootfsHandle, commands: C) -> Self {
        Self {
            handle: Some(handle),
            commands,
        }
    }

    pub(crate) fn handle(&self) -> &TaskRootfsHandle {
        self.handle
            .as_ref()
            .expect("task rootfs lease must hold handle until cleanup or preserve")
    }

    pub(crate) fn cleanup(mut self) -> Result<CleanupResult> {
        let handle = self
            .handle
            .take()
            .expect("task rootfs lease must hold handle until cleanup");
        handle.cleanup_state(&self.commands)
    }

    pub(crate) fn preserve(mut self) -> CleanupResult {
        let handle = self
            .handle
            .take()
            .expect("task rootfs lease must hold handle until preserve");
        CleanupResult::Preserved(handle.rootfs_path)
    }
}

impl<C: BtrfsRootfsCommands> Drop for TaskRootfsLease<C> {
    fn drop(&mut self) {
        let Some(handle) = self.handle.take() else {
            return;
        };
        if handle.preserve_debug {
            return;
        }
        if let Err(err) = cleanup_task_dir(&handle.task_dir, &self.commands) {
            eprintln!(
                "loftd: best-effort task rootfs cleanup failed for '{}': {err:#}",
                handle.task_dir.display()
            );
        }
    }
}

impl TaskRootfsHandle {
    pub(crate) fn task_id(&self) -> &str {
        &self.task_id
    }

    pub(crate) fn task_dir(&self) -> &Path {
        &self.task_dir
    }

    pub(crate) fn rootfs_path(&self) -> &Path {
        &self.rootfs_path
    }

    pub(crate) fn backend(&self) -> TaskRootfsBackend {
        self.backend
    }

    pub(crate) fn selected_image_reference(&self) -> &str {
        &self.selected_image_reference
    }

    pub(crate) fn image_digest(&self) -> Option<&str> {
        self.image_digest.as_deref()
    }

    pub(crate) fn cleanup_state(
        self,
        commands: &impl BtrfsRootfsCommands,
    ) -> Result<CleanupResult> {
        if self.preserve_debug {
            return Ok(CleanupResult::Preserved(self.rootfs_path));
        }
        cleanup_task_dir(&self.task_dir, commands)?;
        Ok(CleanupResult::Removed)
    }

    pub(crate) fn preserve_debug_hint(&self) -> String {
        format!(
            "btrfs snapshot task rootfs is preserved; delete it with `buildah unshare btrfs subvolume delete '{}'` before deleting '{}'. If deletion is denied, inspect the mount with `findmnt -T '{}' -o TARGET,SOURCE,FSTYPE,OPTIONS` and enable the btrfs `user_subvol_rm_allowed` mount option for rootless subvolume cleanup.",
            self.rootfs_path.display(),
            self.task_dir.display(),
            self.rootfs_path.display()
        )
    }
}

#[derive(Debug, Clone)]
pub(crate) struct TaskRootfsManager {
    state_root: PathBuf,
}

impl TaskRootfsManager {
    pub(crate) fn new(state_root: PathBuf) -> Self {
        Self { state_root }
    }

    pub(crate) fn materialize_btrfs_from_buildah(
        &self,
        selection: &ImageSelection,
        task_id: &str,
        preserve_debug: bool,
        buildah: &impl BuildahCommands,
        btrfs: &impl BtrfsRootfsCommands,
    ) -> Result<TaskRootfsHandle> {
        let task_dir = self.state_root.join(TASKS_DIR).join(task_id);
        reset_task_dir(&task_dir, btrfs)?;
        let rootfs_path = task_dir.join(BTRFS_ROOTFS_DIR);

        let source_rootfs =
            image_source::materialize_btrfs_source_rootfs(selection, &rootfs_path, buildah)
                .map_err(|err| {
                    cleanup_after_materialization_failure(&task_dir, btrfs, preserve_debug, err)
                })?;

        Ok(TaskRootfsHandle {
            task_id: task_id.to_owned(),
            task_dir,
            rootfs_path: source_rootfs.rootfs_path,
            backend: TaskRootfsBackend::BtrfsSnapshot,
            selected_image_reference: source_rootfs.selected_reference,
            image_digest: source_rootfs.image_digest,
            preserve_debug,
        })
    }
}

pub(crate) trait BtrfsRootfsCommands {
    fn snapshot_btrfs_subvolume(&self, source: &Path, destination: &Path) -> Result<()>;
    fn delete_btrfs_subvolume(&self, subvolume: &Path) -> Result<()>;
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct HostBtrfsRootfsCommands;

impl BtrfsRootfsCommands for HostBtrfsRootfsCommands {
    fn snapshot_btrfs_subvolume(&self, source: &Path, destination: &Path) -> Result<()> {
        run_buildah_unshare_btrfs_subvolume("snapshot", &[source, destination])
    }

    fn delete_btrfs_subvolume(&self, subvolume: &Path) -> Result<()> {
        run_buildah_unshare_btrfs_subvolume("delete", &[subvolume])
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct UnsharedBtrfsRootfsCommands;

impl BtrfsRootfsCommands for UnsharedBtrfsRootfsCommands {
    fn snapshot_btrfs_subvolume(&self, source: &Path, destination: &Path) -> Result<()> {
        run_btrfs_subvolume("snapshot", &[source, destination])
    }

    fn delete_btrfs_subvolume(&self, subvolume: &Path) -> Result<()> {
        run_btrfs_subvolume("delete", &[subvolume])
    }
}

pub(crate) fn snapshot_mounted_rootfs(
    source: &Path,
    destination: &Path,
    commands: &impl BtrfsRootfsCommands,
) -> Result<()> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create '{}'", parent.display()))?;
    }
    commands
        .snapshot_btrfs_subvolume(source, destination)
        .with_context(|| {
            format!(
                "btrfs-snapshot backend requires Buildah storage and loftd task state on snapshot-compatible btrfs subvolumes; failed to snapshot '{}' to '{}'",
                source.display(),
                destination.display()
            )
        })
}

fn cleanup_after_materialization_failure(
    task_dir: &Path,
    commands: &impl BtrfsRootfsCommands,
    preserve_debug: bool,
    err: anyhow::Error,
) -> anyhow::Error {
    if preserve_debug {
        return err.context(format!(
            "preserved partial loftd task rootfs state at '{}'",
            task_dir.display()
        ));
    }
    match cleanup_task_dir(task_dir, commands) {
        Ok(()) => err,
        Err(cleanup_err) => cleanup_err.context(format!(
            "failed to clean partial loftd task rootfs after materialization error: {err:#}"
        )),
    }
}

fn reset_task_dir(task_dir: &Path, commands: &impl BtrfsRootfsCommands) -> Result<()> {
    if task_dir.exists() {
        cleanup_task_dir(task_dir, commands).with_context(|| {
            format!(
                "failed to reset stale loftd task rootfs directory '{}'",
                task_dir.display()
            )
        })?;
    }
    Ok(())
}

fn cleanup_task_dir(task_dir: &Path, commands: &impl BtrfsRootfsCommands) -> Result<()> {
    if !task_dir.exists() {
        return Ok(());
    }
    let btrfs_rootfs = task_dir.join(BTRFS_ROOTFS_DIR);
    if btrfs_rootfs.exists() {
        commands
            .delete_btrfs_subvolume(&btrfs_rootfs)
            .with_context(|| btrfs_subvolume_delete_failure_hint(&btrfs_rootfs, task_dir))
            .with_context(|| {
                format!(
                    "failed to delete btrfs snapshot task rootfs '{}'",
                    btrfs_rootfs.display()
                )
            })?;
    }
    remove_task_rootfs_tree(task_dir).with_context(|| {
        format!(
            "failed to remove loftd task rootfs directory '{}'",
            task_dir.display()
        )
    })
}

fn run_buildah_unshare_btrfs_subvolume(action: &str, paths: &[&Path]) -> Result<()> {
    let output = Command::new("buildah")
        .arg("unshare")
        .arg("btrfs")
        .arg("subvolume")
        .arg(action)
        .args(paths)
        .stdin(Stdio::null())
        .output()
        .map_err(|err| match err.kind() {
            std::io::ErrorKind::NotFound => anyhow!(
                "buildah is not installed or not available on PATH; btrfs-snapshot cleanup requires buildah unshare"
            ),
            _ => err.into(),
        })
        .with_context(|| format!("failed to run buildah unshare btrfs subvolume {action}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!(
            "buildah unshare btrfs subvolume {action} failed: {}",
            stderr.trim()
        );
    }
    Ok(())
}

fn run_btrfs_subvolume(action: &str, paths: &[&Path]) -> Result<()> {
    let output = Command::new("btrfs")
        .arg("subvolume")
        .arg(action)
        .args(paths)
        .stdin(Stdio::null())
        .output()
        .map_err(|err| match err.kind() {
            std::io::ErrorKind::NotFound => {
                anyhow!("btrfs is not installed or not available on PATH")
            }
            _ => err.into(),
        })
        .with_context(|| format!("failed to run btrfs subvolume {action}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("btrfs subvolume {action} failed: {}", stderr.trim());
    }
    Ok(())
}

fn btrfs_subvolume_delete_failure_hint(rootfs: &Path, task_dir: &Path) -> String {
    format!(
        "btrfs-snapshot cleanup uses `btrfs subvolume delete`, which is fast but requires root/CAP_SYS_ADMIN unless the host btrfs mount allows user-owned subvolume removal. For rootless cleanup, enable the btrfs `user_subvol_rm_allowed` mount option on the filesystem containing this task rootfs. Inspect the relevant mount with `findmnt -T '{}' -o TARGET,SOURCE,FSTYPE,OPTIONS`; add `user_subvol_rm_allowed` to the OPTIONS for the matching btrfs entry in /etc/fstab; then remount with `sudo mount -o remount,user_subvol_rm_allowed <mountpoint>`. After remounting, retry manual cleanup with `buildah unshare btrfs subvolume delete '{}'` and then remove the task state dir '{}' if it remains",
        rootfs.display(),
        rootfs.display(),
        task_dir.display()
    )
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
                "failed to make loftd task rootfs directory writable '{}'",
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

pub(crate) fn new_task_id(workspace_slug: &str) -> String {
    format!(
        "{}-{}-{}",
        workspace_slug,
        std::process::id(),
        monotonic_nanos()
    )
}

fn monotonic_nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0)
}

#[cfg(test)]
fn copy_rootfs_tree(source: &Path, destination: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(source)
        .with_context(|| format!("failed to stat '{}'", source.display()))?;
    if metadata.is_dir() {
        fs::create_dir_all(destination)
            .with_context(|| format!("failed to create '{}'", destination.display()))?;
        for entry in fs::read_dir(source)
            .with_context(|| format!("failed to read '{}'", source.display()))?
        {
            let entry = entry?;
            copy_rootfs_tree(&entry.path(), &destination.join(entry.file_name()))?;
        }
        fs::set_permissions(
            destination,
            fs::Permissions::from_mode(metadata.permissions().mode()),
        )?;
    } else if metadata.is_file() {
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(source, destination)?;
        fs::set_permissions(
            destination,
            fs::Permissions::from_mode(metadata.permissions().mode()),
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
