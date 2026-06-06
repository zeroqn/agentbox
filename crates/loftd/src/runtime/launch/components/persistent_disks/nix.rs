//! Persistent Nix cache disk contribution.
//!
//! This file owns only the existing Nix disk contribution; it does not define
//! new disk policy, env keys, labels, ids, or validation behavior.

use anyhow::{Context, Result};
use std::path::Path;

use crate::runtime::launch::components::persistent_disks::raw_btrfs::{
    self, RawBtrfsDisk, RawDiskSpec,
};
use crate::runtime::launch::config::DiskAttachment;

pub(super) const FILE_NAME: &str = "loftd-nix.raw";
pub(super) const ID: &str = "loftd-nix";
pub(super) const LABEL: &str = "LOFTD_NIX";
pub(super) const SIZE_BYTES: u64 = 64 * 1024 * 1024 * 1024;

const SPEC: RawDiskSpec = RawDiskSpec {
    file_name: FILE_NAME,
    id: ID,
    label: LABEL,
    size_bytes: SIZE_BYTES,
    size_hint: "64G",
    diagnostic_name: "loftd persistent /nix dev cache disk",
};

pub(super) fn prepare_with_runner(
    state_root: &Path,
    runner: &impl raw_btrfs::RawImageCommandRunner,
) -> Result<RawBtrfsDisk> {
    raw_btrfs::prepare_with_runner(state_root, &SPEC, runner)
        .context("failed to prepare loftd persistent /nix dev cache disk")
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
        ("LOFTD_NIX_OVERLAY".to_owned(), "1".to_owned()),
        ("LOFTD_NIX_DISK_ID".to_owned(), disk.id.clone()),
        ("LOFTD_NIX_DISK_LABEL".to_owned(), disk.label.clone()),
    ]
}
