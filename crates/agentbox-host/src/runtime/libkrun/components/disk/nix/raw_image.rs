use anyhow::Result;
use std::path::Path;

pub(crate) use crate::runtime::libkrun::components::disk::raw_btrfs::RawBtrfsDisk as RawNixDisk;
use crate::runtime::libkrun::components::disk::raw_btrfs::{self, RawDiskSpec};
#[cfg(test)]
pub(crate) use crate::runtime::libkrun::components::disk::raw_btrfs::{
    RawBtrfsDiskStatus as RawNixDiskStatus, RawImageCommandRunner,
};

pub(crate) const RAW_NIX_DISK_FILE_NAME: &str = "libkrun-nix.raw";
pub(crate) const RAW_NIX_DISK_ID: &str = "agentbox-nix";
pub(crate) const RAW_NIX_DISK_LABEL: &str = "AGENTBOX_NIX";
pub(crate) const RAW_NIX_DISK_SIZE_BYTES: u64 = 64 * 1024 * 1024 * 1024;

pub(crate) const RAW_NIX_DISK_SPEC: RawDiskSpec = RawDiskSpec {
    file_name: RAW_NIX_DISK_FILE_NAME,
    id: RAW_NIX_DISK_ID,
    label: RAW_NIX_DISK_LABEL,
    size_bytes: RAW_NIX_DISK_SIZE_BYTES,
    size_hint: "64G",
    diagnostic_name: "libkrun /nix raw image",
};

pub(crate) fn prepare(state_root: &Path) -> Result<RawNixDisk> {
    raw_btrfs::prepare(state_root, &RAW_NIX_DISK_SPEC)
}

#[cfg(test)]
pub(crate) fn prepare_with_runner(
    state_root: &Path,
    runner: &impl RawImageCommandRunner,
) -> Result<RawNixDisk> {
    raw_btrfs::prepare_with_runner(state_root, &RAW_NIX_DISK_SPEC, runner)
}

#[cfg(test)]
mod tests {
    use crate::runtime::libkrun::components::disk::nix::raw_image::{
        RAW_NIX_DISK_FILE_NAME, RAW_NIX_DISK_ID, RAW_NIX_DISK_LABEL, RAW_NIX_DISK_SIZE_BYTES,
        RawNixDiskStatus, prepare_with_runner,
    };
    use crate::runtime::libkrun::components::disk::raw_btrfs::test_support::FakeRunner;
    use std::fs::{self, File};
    use tempfile::tempdir;

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

        assert!(err.to_string().contains("libkrun /nix raw image"));
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
