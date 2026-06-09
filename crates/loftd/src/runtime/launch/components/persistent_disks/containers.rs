//! Persistent container-store disk contribution.
//!
//! This file owns only the existing containers disk contribution; it does not
//! define new disk policy, env keys, labels, ids, or validation behavior.

use anyhow::{Context, Result};
use std::path::Path;

use crate::runtime::launch::components::persistent_disks::raw_btrfs::{
    self, RawBtrfsDisk, RawDiskSpec,
};
use crate::runtime::launch::config::DiskAttachment;

pub(super) const FILE_NAME: &str = "loftd-containers.raw";
pub(super) const ID: &str = "loftd-containers";
pub(super) const LABEL: &str = "LOFTD_CONTAINERS";
pub(super) const SIZE_BYTES: u64 = 64 * 1024 * 1024 * 1024;
pub(super) const STORAGE_ENV: &str = "LOFTD_CONTAINERS_STORAGE";
pub(super) const STORE_ENV: &str = "LOFTD_CONTAINERS_STORE";
pub(super) const STORE_RAW_DISK: &str = "raw-disk";

const SPEC: RawDiskSpec = RawDiskSpec {
    file_name: FILE_NAME,
    id: ID,
    label: LABEL,
    size_bytes: SIZE_BYTES,
    size_hint: "64G",
    diagnostic_name: "loftd persistent container-store dev cache disk",
};

pub(super) fn prepare_with_runner(
    state_root: &Path,
    runner: &impl raw_btrfs::RawImageCommandRunner,
) -> Result<RawBtrfsDisk> {
    raw_btrfs::prepare_with_runner(state_root, &SPEC, runner)
        .context("failed to prepare loftd persistent container-store dev cache disk")
}

pub(crate) fn grow_with_runner(
    state_root: &Path,
    target_size_bytes: u64,
    runner: &impl raw_btrfs::RawImageCommandRunner,
) -> Result<RawBtrfsDisk> {
    raw_btrfs::grow_existing_with_runner(state_root, &SPEC, target_size_bytes, runner)
        .context("failed to grow loftd persistent container-store dev cache disk")
}

pub(crate) fn recreate_with_runner(
    state_root: &Path,
    runner: &impl raw_btrfs::RawImageCommandRunner,
) -> Result<RawBtrfsDisk> {
    raw_btrfs::recreate_with_runner(state_root, &SPEC, runner)
        .context("failed to reset loftd persistent container-store dev cache disk")
}

pub(crate) fn default_size_bytes() -> u64 {
    SIZE_BYTES
}

pub(super) fn attachment(disk: &RawBtrfsDisk) -> DiskAttachment {
    DiskAttachment {
        id: disk.id.clone(),
        path: disk.path.clone(),
        read_only: false,
    }
}

pub(super) fn raw_disk_env_pairs(disk: &RawBtrfsDisk) -> [(String, String); 4] {
    [
        (STORAGE_ENV.to_owned(), "1".to_owned()),
        (STORE_ENV.to_owned(), STORE_RAW_DISK.to_owned()),
        ("LOFTD_CONTAINERS_DISK_ID".to_owned(), disk.id.clone()),
        ("LOFTD_CONTAINERS_DISK_LABEL".to_owned(), disk.label.clone()),
    ]
}
