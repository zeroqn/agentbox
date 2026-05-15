use anyhow::Result;
use std::path::Path;

pub(crate) use crate::runtime::libkrun::components::disk::raw_btrfs::RawBtrfsDisk as RawContainerDisk;
use crate::runtime::libkrun::components::disk::raw_btrfs::{self, RawDiskSpec};
#[cfg(test)]
pub(crate) use crate::runtime::libkrun::components::disk::raw_btrfs::{
    RawBtrfsDiskStatus as RawContainerDiskStatus, RawImageCommandRunner,
};

pub(crate) const RAW_CONTAINER_DISK_FILE_NAME: &str = "libkrun-containers.raw";
pub(crate) const RAW_CONTAINER_DISK_ID: &str = "agentbox-containers";
pub(crate) const RAW_CONTAINER_DISK_LABEL: &str = "AGENTBOX_CONTAINERS";
pub(crate) const RAW_CONTAINER_DISK_SIZE_BYTES: u64 = 64 * 1024 * 1024 * 1024;

const RAW_CONTAINER_DISK_SPEC: RawDiskSpec = RawDiskSpec {
    file_name: RAW_CONTAINER_DISK_FILE_NAME,
    id: RAW_CONTAINER_DISK_ID,
    label: RAW_CONTAINER_DISK_LABEL,
    size_bytes: RAW_CONTAINER_DISK_SIZE_BYTES,
    size_hint: "64G",
    diagnostic_name: "libkrun container storage raw image",
};

pub(crate) fn prepare(state_root: &Path) -> Result<RawContainerDisk> {
    raw_btrfs::prepare(state_root, &RAW_CONTAINER_DISK_SPEC)
}

#[cfg(test)]
pub(crate) fn prepare_with_runner(
    state_root: &Path,
    runner: &impl RawImageCommandRunner,
) -> Result<RawContainerDisk> {
    raw_btrfs::prepare_with_runner(state_root, &RAW_CONTAINER_DISK_SPEC, runner)
}

#[cfg(test)]
mod tests {
    use crate::runtime::libkrun::components::disk::containers::raw_image::{
        RAW_CONTAINER_DISK_FILE_NAME, RAW_CONTAINER_DISK_ID, RAW_CONTAINER_DISK_LABEL,
        RAW_CONTAINER_DISK_SIZE_BYTES, RawContainerDiskStatus, prepare_with_runner,
    };
    use crate::runtime::libkrun::components::disk::raw_btrfs::test_support::FakeRunner;
    use std::fs::{self, File};
    use tempfile::tempdir;

    #[test]
    fn missing_container_image_creates_sparse_btrfs_file() {
        let temp = tempdir().unwrap();
        let runner = FakeRunner::default();

        let disk = prepare_with_runner(temp.path(), &runner).unwrap();

        assert_eq!(disk.status, RawContainerDiskStatus::Created);
        assert_eq!(disk.path, temp.path().join(RAW_CONTAINER_DISK_FILE_NAME));
        assert_eq!(disk.id, RAW_CONTAINER_DISK_ID);
        assert_eq!(disk.label, RAW_CONTAINER_DISK_LABEL);
        assert_eq!(disk.size_bytes, RAW_CONTAINER_DISK_SIZE_BYTES);
        assert_eq!(
            disk.path.metadata().unwrap().len(),
            RAW_CONTAINER_DISK_SIZE_BYTES
        );
        assert_eq!(runner.mkfs_call_count(), 1);
        assert_eq!(runner.mkfs_calls.borrow()[0].1, RAW_CONTAINER_DISK_LABEL);
    }

    #[test]
    fn existing_invalid_container_image_has_container_storage_diagnostic() {
        let temp = tempdir().unwrap();
        let path = temp.path().join(RAW_CONTAINER_DISK_FILE_NAME);
        let file = File::create(&path).unwrap();
        file.set_len(RAW_CONTAINER_DISK_SIZE_BYTES).unwrap();
        drop(file);
        let runner = FakeRunner::with_probe("ext4");

        let err = prepare_with_runner(temp.path(), &runner).unwrap_err();

        assert!(err.to_string().contains("container storage raw image"));
        assert!(err.to_string().contains("expected btrfs"));
        assert!(err.to_string().contains("refusing to reformat"));
        assert!(!err.to_string().contains("libkrun /nix raw image"));
        assert_eq!(runner.mkfs_call_count(), 0);
    }

    #[test]
    fn existing_too_small_container_image_fails_before_probe_or_format() {
        let temp = tempdir().unwrap();
        let path = temp.path().join(RAW_CONTAINER_DISK_FILE_NAME);
        let file = File::create(&path).unwrap();
        file.set_len(RAW_CONTAINER_DISK_SIZE_BYTES - 1).unwrap();
        drop(file);
        let runner = FakeRunner::default();

        let err = prepare_with_runner(temp.path(), &runner).unwrap_err();

        assert!(err.to_string().contains("container storage raw image"));
        assert!(err.to_string().contains("below the required"));
        assert!(err.to_string().contains("truncate -s 64G"));
        assert_eq!(runner.mkfs_call_count(), 0);
    }

    #[test]
    fn existing_container_directory_is_not_overwritten() {
        let temp = tempdir().unwrap();
        fs::create_dir(temp.path().join(RAW_CONTAINER_DISK_FILE_NAME)).unwrap();
        let runner = FakeRunner::default();

        let err = prepare_with_runner(temp.path(), &runner).unwrap_err();

        assert!(err.to_string().contains("container storage raw image"));
        assert!(err.to_string().contains("not a regular file"));
        assert!(err.to_string().contains("refusing to overwrite"));
    }
}
