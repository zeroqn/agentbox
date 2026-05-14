use anyhow::{Context, Result};
use std::path::PathBuf;

const PREFERRED_NIX_DISK: &str = "/dev/vda";

pub(in crate::guest_init) fn find_disk(label: &str, disk_id: &str) -> Result<PathBuf> {
    crate::guest_init::components::disk::btrfs::find_labeled_disk_with_preferred(
        label,
        disk_id,
        &[PREFERRED_NIX_DISK],
    )
    .with_context(|| format!("libkrun /nix btrfs disk not found (label={label} id={disk_id})"))
}
