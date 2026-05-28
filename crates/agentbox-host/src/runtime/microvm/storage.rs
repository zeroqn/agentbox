use anyhow::{Context, Result, anyhow};
use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};

use crate::cli::MicrovmStoragePolicy;
use crate::runtime::microvm::image_cache::ImageCacheEntry;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StorageBackend {
    Btrfs,
    FuseOverlay,
}

pub(crate) trait StorageProbe {
    fn btrfs_available(&self) -> bool;
    fn fuse_overlay_available(&self) -> bool;
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct HostStorageProbe;

impl StorageProbe for HostStorageProbe {
    fn btrfs_available(&self) -> bool {
        command_available("btrfs")
    }

    fn fuse_overlay_available(&self) -> bool {
        command_available("fuse-overlayfs")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TaskRootfsHandle {
    pub(crate) root: PathBuf,
    task_dir: PathBuf,
    preserve_debug: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CleanupResult {
    Removed,
    Preserved(PathBuf),
}

impl TaskRootfsHandle {
    pub(crate) fn cleanup(self) -> Result<CleanupResult> {
        if self.preserve_debug {
            return Ok(CleanupResult::Preserved(self.root));
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
        probe: &impl StorageProbe,
    ) -> Result<StorageBackend> {
        match policy {
            MicrovmStoragePolicy::Auto if probe.btrfs_available() => Ok(StorageBackend::Btrfs),
            MicrovmStoragePolicy::Auto if probe.fuse_overlay_available() => {
                Ok(StorageBackend::FuseOverlay)
            }
            MicrovmStoragePolicy::Auto => Err(anyhow!(
                "experimental microvm storage requires btrfs or fuse-overlayfs; install btrfs-progs or fuse-overlayfs"
            )),
            MicrovmStoragePolicy::Btrfs if probe.btrfs_available() => Ok(StorageBackend::Btrfs),
            MicrovmStoragePolicy::Btrfs => Err(anyhow!(
                "experimental microvm storage --storage btrfs requires btrfs-progs/btrfs on PATH"
            )),
            MicrovmStoragePolicy::FuseOverlay if probe.fuse_overlay_available() => {
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
    ) -> Result<TaskRootfsHandle> {
        entry.ensure_agentbox_compatible()?;
        let task_dir = self.state_root.join("microvm-tasks").join(task_id);
        let root = task_dir.join(match backend {
            StorageBackend::Btrfs => "rootfs-btrfs",
            StorageBackend::FuseOverlay => "rootfs-fuse-overlay",
        });
        if root.exists() {
            remove_task_rootfs_tree(&root).with_context(|| {
                format!(
                    "failed to reset stale microvm task rootfs '{}'",
                    root.display()
                )
            })?;
        }
        copy_rootfs_tree(&entry.rootfs, &root).with_context(|| {
            format!(
                "failed to materialize microvm task rootfs from '{}' to '{}'",
                entry.rootfs.display(),
                root.display()
            )
        })?;
        Ok(TaskRootfsHandle {
            root,
            task_dir,
            preserve_debug,
        })
    }
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

    #[derive(Debug, Clone, Copy)]
    struct Probe {
        btrfs: bool,
        fuse: bool,
    }

    impl StorageProbe for Probe {
        fn btrfs_available(&self) -> bool {
            self.btrfs
        }

        fn fuse_overlay_available(&self) -> bool {
            self.fuse
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
    fn auto_storage_prefers_btrfs_then_falls_back_to_fuse_overlay() {
        assert_eq!(
            StorageManager::select_backend(
                MicrovmStoragePolicy::Auto,
                &Probe {
                    btrfs: true,
                    fuse: true,
                }
            )
            .expect("auto should select btrfs when available"),
            StorageBackend::Btrfs
        );
        assert_eq!(
            StorageManager::select_backend(
                MicrovmStoragePolicy::Auto,
                &Probe {
                    btrfs: false,
                    fuse: true,
                }
            )
            .expect("auto should fall back to fuse-overlay"),
            StorageBackend::FuseOverlay
        );
    }

    #[test]
    fn explicit_storage_policy_reports_missing_helper() {
        let btrfs_error = StorageManager::select_backend(
            MicrovmStoragePolicy::Btrfs,
            &Probe {
                btrfs: false,
                fuse: true,
            },
        )
        .expect_err("explicit btrfs should fail when unavailable");
        assert!(format!("{btrfs_error:#}").contains("btrfs"));

        let fuse_error = StorageManager::select_backend(
            MicrovmStoragePolicy::FuseOverlay,
            &Probe {
                btrfs: true,
                fuse: false,
            },
        )
        .expect_err("explicit fuse-overlay should fail when unavailable");
        assert!(format!("{fuse_error:#}").contains("fuse-overlayfs"));
    }

    #[test]
    fn task_rootfs_is_writable_copy_and_cleanup_removes_it() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let entry = cached_entry(&temp);
        let manager = StorageManager::new(temp.path().join("workspace-state"));

        let handle = manager
            .materialize(&entry, StorageBackend::FuseOverlay, "task-1", false)
            .expect("task rootfs should materialize");

        assert_ne!(handle.root, entry.rootfs);
        assert_eq!(
            fs::read_to_string(handle.root.join("etc").join("agentbox"))
                .expect("cached content should be visible"),
            "cached"
        );
        fs::write(handle.root.join("task-only"), "mutable").expect("task root should be writable");
        fs::write(handle.task_dir.join("launch.conf"), "config")
            .expect("launch config should be written");
        assert!(!entry.rootfs.join("task-only").exists());
        let root = handle.root.clone();
        let task_dir = handle.task_dir.clone();

        assert_eq!(
            handle.cleanup().expect("cleanup should succeed"),
            CleanupResult::Removed
        );
        assert!(!root.exists());
        assert!(!task_dir.exists());
    }

    #[test]
    fn cleanup_removes_task_rootfs_with_read_only_directories() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let entry = cached_entry(&temp);
        let read_only_dir = entry.rootfs.join("nix/store/hash/bin");
        fs::create_dir_all(&read_only_dir).expect("read-only dir should be created");
        fs::write(read_only_dir.join("tool"), "#!/bin/sh\n").expect("tool should be written");
        set_mode(&read_only_dir, 0o555);
        let manager = StorageManager::new(temp.path().join("workspace-state"));

        let handle = manager
            .materialize(
                &entry,
                StorageBackend::FuseOverlay,
                "readonly-cleanup",
                false,
            )
            .expect("task rootfs should materialize");
        set_mode(&read_only_dir, 0o755);
        fs::write(handle.task_dir.join("launch.conf"), "config")
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
        let task_dir = handle.task_dir.clone();

        assert_eq!(
            handle
                .cleanup()
                .expect("cleanup should remove read-only dirs"),
            CleanupResult::Removed
        );
        assert!(!root.exists());
        assert!(!task_dir.exists());
    }

    #[test]
    fn materialize_resets_stale_read_only_task_rootfs() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let entry = cached_entry(&temp);
        let read_only_dir = entry.rootfs.join("nix/store/hash/bin");
        fs::create_dir_all(&read_only_dir).expect("read-only dir should be created");
        fs::write(read_only_dir.join("tool"), "#!/bin/sh\n").expect("tool should be written");
        set_mode(&read_only_dir, 0o555);
        let manager = StorageManager::new(temp.path().join("workspace-state"));

        let stale = manager
            .materialize(&entry, StorageBackend::FuseOverlay, "readonly-reset", true)
            .expect("stale task rootfs should materialize");
        assert!(stale.root.exists());
        set_mode(&read_only_dir, 0o755);

        let replacement = manager
            .materialize(&entry, StorageBackend::FuseOverlay, "readonly-reset", false)
            .expect("stale read-only task rootfs should be reset");
        let replacement_root = replacement.root.clone();

        assert!(replacement_root.exists());
        assert_eq!(
            replacement
                .cleanup()
                .expect("replacement cleanup should succeed"),
            CleanupResult::Removed
        );
    }

    #[test]
    fn task_rootfs_materialization_preserves_executable_modes_and_symlinks() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let cache = ImageCache::new(temp.path().join("images"));
        let digest = ImageDigest::parse("sha256:abc123").expect("digest should parse");
        let entry_dir = cache.entry_path(&digest);
        let bin_dir = entry_dir.join("rootfs").join("usr/bin");
        fs::create_dir_all(&bin_dir).expect("bin dir should be created");
        let tool = bin_dir.join("tool");
        fs::write(&tool, "#!/bin/sh\n").expect("tool should be written");
        fs::set_permissions(&tool, fs::Permissions::from_mode(0o755))
            .expect("tool mode should be set");
        std::os::unix::fs::symlink("tool", bin_dir.join("tool-link"))
            .expect("symlink should be created");
        fs::write(entry_dir.join("agentbox-compatible"), "agentbox\n")
            .expect("compatibility marker should be written");
        let entry = cache
            .ensure(
                ImageReference::from_cli(Some("ghcr.io/example/agentbox@sha256:abc123")),
                &NoBuildah,
            )
            .expect("cache hit should resolve");
        let manager = StorageManager::new(temp.path().join("workspace-state"));

        let handle = manager
            .materialize(&entry, StorageBackend::FuseOverlay, "task-preserve", false)
            .expect("task rootfs should materialize");

        assert_eq!(
            fs::metadata(handle.root.join("usr/bin/tool"))
                .expect("tool metadata should be readable")
                .permissions()
                .mode()
                & 0o777,
            0o755
        );
        assert_eq!(
            fs::read_link(handle.root.join("usr/bin/tool-link"))
                .expect("symlink should be preserved"),
            PathBuf::from("tool")
        );
    }

    #[test]
    fn preserve_debug_keeps_task_rootfs_and_reports_path() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let entry = cached_entry(&temp);
        let manager = StorageManager::new(temp.path().join("workspace-state"));
        let handle = manager
            .materialize(&entry, StorageBackend::Btrfs, "task-2", true)
            .expect("task rootfs should materialize");
        let root = handle.root.clone();

        assert_eq!(
            handle.cleanup().expect("cleanup should preserve"),
            CleanupResult::Preserved(root.clone())
        );
        assert!(root.exists());
    }
}
