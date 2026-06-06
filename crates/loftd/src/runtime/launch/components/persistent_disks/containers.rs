//! Persistent container-store disk contribution.
//!
//! This file owns only the existing containers disk contribution; it does not
//! define new disk policy, env keys, labels, ids, or validation behavior.

use anyhow::{Context, Result};
use std::path::Path;

use crate::runtime::launch::config::DiskAttachment;
use crate::runtime::raw_btrfs::{self, RawBtrfsDisk, RawDiskSpec};

pub(super) const FILE_NAME: &str = "loftd-containers.raw";
pub(super) const ID: &str = "loftd-containers";
pub(super) const LABEL: &str = "LOFTD_CONTAINERS";
pub(super) const SIZE_BYTES: u64 = 64 * 1024 * 1024 * 1024;

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

pub(super) fn attachment(disk: &RawBtrfsDisk) -> DiskAttachment {
    DiskAttachment {
        id: disk.id.clone(),
        path: disk.path.clone(),
        read_only: false,
    }
}

pub(super) fn env_pairs(disk: &RawBtrfsDisk) -> [(String, String); 3] {
    [
        ("LOFTD_CONTAINERS_STORAGE".to_owned(), "1".to_owned()),
        ("LOFTD_CONTAINERS_DISK_ID".to_owned(), disk.id.clone()),
        ("LOFTD_CONTAINERS_DISK_LABEL".to_owned(), disk.label.clone()),
    ]
}
