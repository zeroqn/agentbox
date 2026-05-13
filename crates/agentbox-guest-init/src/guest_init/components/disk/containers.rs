use anyhow::{Context, Result};
use std::path::PathBuf;

pub(in crate::guest_init) fn find_disk(label: &str, disk_id: &str) -> Result<PathBuf> {
    crate::guest_init::components::disk::btrfs::find_labeled_disk(label, disk_id).with_context(
        || format!("libkrun container storage btrfs disk not found (label={label} id={disk_id})"),
    )
}
