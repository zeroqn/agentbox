use anyhow::{anyhow, Context, Result};
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::Command;

pub(crate) const RAW_NIX_DISK_FILE_NAME: &str = "libkrun-nix.raw";
pub(crate) const RAW_NIX_DISK_ID: &str = "agentbox-nix";
pub(crate) const RAW_NIX_DISK_LABEL: &str = "AGENTBOX_NIX";
pub(crate) const RAW_NIX_DISK_SIZE_BYTES: u64 = 64 * 1024 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RawNixDisk {
    pub(crate) path: PathBuf,
    pub(crate) id: String,
    pub(crate) label: String,
    pub(crate) size_bytes: u64,
    pub(crate) status: RawNixDiskStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RawNixDiskStatus {
    Created,
    Reused,
}

pub(crate) trait RawImageCommandRunner {
    fn mkfs_btrfs(&self, path: &Path, label: &str) -> Result<()>;
    fn probe_fs_type(&self, path: &Path) -> Result<String>;
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct HostRawImageCommandRunner;

impl RawImageCommandRunner for HostRawImageCommandRunner {
    fn mkfs_btrfs(&self, path: &Path, label: &str) -> Result<()> {
        let output = Command::new("mkfs.btrfs")
            .arg("-f")
            .arg("-L")
            .arg(label)
            .arg(path)
            .output()
            .map_err(|err| match err.kind() {
                std::io::ErrorKind::NotFound => anyhow!(
                    "mkfs.btrfs is required to create the libkrun /nix raw image; install btrfs-progs and retry"
                ),
                _ => err.into(),
            })
            .with_context(|| format!("failed to run mkfs.btrfs for '{}'", path.display()))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            if stderr.is_empty() {
                anyhow::bail!("mkfs.btrfs failed for '{}'", path.display());
            }
            anyhow::bail!("mkfs.btrfs failed for '{}': {stderr}", path.display());
        }

        Ok(())
    }

    fn probe_fs_type(&self, path: &Path) -> Result<String> {
        let output = Command::new("blkid")
            .arg("-o")
            .arg("value")
            .arg("-s")
            .arg("TYPE")
            .arg(path)
            .output()
            .map_err(|err| match err.kind() {
                std::io::ErrorKind::NotFound => anyhow!(
                    "blkid is required to validate the existing libkrun /nix raw image; install util-linux and retry"
                ),
                _ => err.into(),
            })
            .with_context(|| format!("failed to run blkid for '{}'", path.display()))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            if stderr.is_empty() {
                anyhow::bail!("failed to detect filesystem type for '{}'", path.display());
            }
            anyhow::bail!(
                "failed to detect filesystem type for '{}': {stderr}",
                path.display()
            );
        }

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    }
}

pub(crate) fn prepare(state_root: &Path) -> Result<RawNixDisk> {
    prepare_with_runner(state_root, &HostRawImageCommandRunner)
}

pub(crate) fn prepare_with_runner(
    state_root: &Path,
    runner: &impl RawImageCommandRunner,
) -> Result<RawNixDisk> {
    let path = state_root.join(RAW_NIX_DISK_FILE_NAME);
    prepare_path_with_runner(&path, runner)
}

pub(crate) fn prepare_path_with_runner(
    path: &Path,
    runner: &impl RawImageCommandRunner,
) -> Result<RawNixDisk> {
    if path.exists() {
        validate_existing(path, runner)?;
        return Ok(disk(path.to_path_buf(), RawNixDiskStatus::Reused));
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create '{}'", parent.display()))?;
    }

    let file = File::create_new(path).with_context(|| {
        format!(
            "failed to create libkrun /nix raw image '{}'",
            path.display()
        )
    })?;
    file.set_len(RAW_NIX_DISK_SIZE_BYTES).with_context(|| {
        format!(
            "failed to set libkrun /nix raw image '{}' to {} bytes",
            path.display(),
            RAW_NIX_DISK_SIZE_BYTES
        )
    })?;
    drop(file);

    if let Err(err) = runner.mkfs_btrfs(path, RAW_NIX_DISK_LABEL) {
        let _ = fs::remove_file(path);
        return Err(err).with_context(|| {
            format!(
                "failed to format new libkrun /nix raw image '{}' as btrfs",
                path.display()
            )
        });
    }

    Ok(disk(path.to_path_buf(), RawNixDiskStatus::Created))
}

fn validate_existing(path: &Path, runner: &impl RawImageCommandRunner) -> Result<()> {
    let metadata = path
        .metadata()
        .with_context(|| format!("failed to inspect '{}'", path.display()))?;
    if !metadata.is_file() {
        anyhow::bail!(
            "existing libkrun /nix raw image path '{}' is not a regular file; refusing to overwrite it",
            path.display()
        );
    }

    if metadata.len() < RAW_NIX_DISK_SIZE_BYTES {
        anyhow::bail!(
            "existing libkrun /nix raw image '{}' is {} bytes, below the required {} bytes; stop the VM, extend it with 'truncate -s 64G {}', then retry",
            path.display(),
            metadata.len(),
            RAW_NIX_DISK_SIZE_BYTES,
            path.display()
        );
    }

    let fs_type = runner.probe_fs_type(path)?;
    if fs_type != "btrfs" {
        anyhow::bail!(
            "existing libkrun /nix raw image '{}' has filesystem type '{}', expected btrfs; refusing to reformat automatically",
            path.display(),
            if fs_type.is_empty() { "unknown" } else { &fs_type }
        );
    }

    Ok(())
}

fn disk(path: PathBuf, status: RawNixDiskStatus) -> RawNixDisk {
    RawNixDisk {
        path,
        id: RAW_NIX_DISK_ID.to_owned(),
        label: RAW_NIX_DISK_LABEL.to_owned(),
        size_bytes: RAW_NIX_DISK_SIZE_BYTES,
        status,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use tempfile::tempdir;

    #[derive(Default)]
    struct FakeRunner {
        mkfs_calls: RefCell<Vec<(PathBuf, String)>>,
        probe_results: RefCell<VecDeque<Result<String, String>>>,
    }

    impl FakeRunner {
        fn with_probe(fs_type: &str) -> Self {
            let runner = Self::default();
            runner
                .probe_results
                .borrow_mut()
                .push_back(Ok(fs_type.to_owned()));
            runner
        }

        fn mkfs_call_count(&self) -> usize {
            self.mkfs_calls.borrow().len()
        }
    }

    impl RawImageCommandRunner for FakeRunner {
        fn mkfs_btrfs(&self, path: &Path, label: &str) -> Result<()> {
            self.mkfs_calls
                .borrow_mut()
                .push((path.to_path_buf(), label.to_owned()));
            Ok(())
        }

        fn probe_fs_type(&self, _path: &Path) -> Result<String> {
            match self.probe_results.borrow_mut().pop_front() {
                Some(Ok(value)) => Ok(value),
                Some(Err(message)) => Err(anyhow!(message)),
                None => Err(anyhow!("unexpected probe")),
            }
        }
    }

    #[test]
    fn missing_image_creates_sparse_btrfs_file() {
        let temp = tempdir().unwrap();
        let runner = FakeRunner::default();

        let disk = prepare_with_runner(temp.path(), &runner).unwrap();

        assert_eq!(disk.status, RawNixDiskStatus::Created);
        assert_eq!(disk.path, temp.path().join(RAW_NIX_DISK_FILE_NAME));
        assert_eq!(disk.id, RAW_NIX_DISK_ID);
        assert_eq!(disk.label, RAW_NIX_DISK_LABEL);
        assert_eq!(disk.size_bytes, RAW_NIX_DISK_SIZE_BYTES);
        assert_eq!(disk.path.metadata().unwrap().len(), RAW_NIX_DISK_SIZE_BYTES);
        assert_eq!(runner.mkfs_call_count(), 1);
        assert_eq!(runner.mkfs_calls.borrow()[0].1, RAW_NIX_DISK_LABEL);
    }

    #[test]
    fn existing_valid_btrfs_image_is_reused_without_formatting() {
        let temp = tempdir().unwrap();
        let path = temp.path().join(RAW_NIX_DISK_FILE_NAME);
        let file = File::create(&path).unwrap();
        file.set_len(RAW_NIX_DISK_SIZE_BYTES).unwrap();
        drop(file);
        let runner = FakeRunner::with_probe("btrfs");

        let disk = prepare_with_runner(temp.path(), &runner).unwrap();

        assert_eq!(disk.status, RawNixDiskStatus::Reused);
        assert_eq!(disk.path, path);
        assert_eq!(runner.mkfs_call_count(), 0);
    }

    #[test]
    fn existing_invalid_image_fails_without_reformatting() {
        let temp = tempdir().unwrap();
        let path = temp.path().join(RAW_NIX_DISK_FILE_NAME);
        let file = File::create(&path).unwrap();
        file.set_len(RAW_NIX_DISK_SIZE_BYTES).unwrap();
        drop(file);
        let runner = FakeRunner::with_probe("ext4");

        let err = prepare_with_runner(temp.path(), &runner).unwrap_err();

        assert!(err.to_string().contains("expected btrfs"));
        assert!(err.to_string().contains("refusing to reformat"));
        assert_eq!(runner.mkfs_call_count(), 0);
    }

    #[test]
    fn existing_too_small_image_fails_before_probe_or_format() {
        let temp = tempdir().unwrap();
        let path = temp.path().join(RAW_NIX_DISK_FILE_NAME);
        let file = File::create(&path).unwrap();
        file.set_len(RAW_NIX_DISK_SIZE_BYTES - 1).unwrap();
        drop(file);
        let runner = FakeRunner::default();

        let err = prepare_with_runner(temp.path(), &runner).unwrap_err();

        assert!(err.to_string().contains("below the required"));
        assert!(err.to_string().contains("truncate -s 64G"));
        assert_eq!(runner.mkfs_call_count(), 0);
    }

    #[test]
    fn existing_directory_is_not_overwritten() {
        let temp = tempdir().unwrap();
        fs::create_dir(temp.path().join(RAW_NIX_DISK_FILE_NAME)).unwrap();
        let runner = FakeRunner::default();

        let err = prepare_with_runner(temp.path(), &runner).unwrap_err();

        assert!(err.to_string().contains("not a regular file"));
        assert!(err.to_string().contains("refusing to overwrite"));
    }
}
