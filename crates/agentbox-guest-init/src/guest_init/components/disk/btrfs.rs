use anyhow::{anyhow, Context, Result};
use std::path::PathBuf;
use std::process::Command;

use crate::guest_init::command;

pub(in crate::guest_init) fn find_labeled_disk(label: &str, disk_id: &str) -> Result<PathBuf> {
    if let Some(path) = command::output_trimmed("blkid", &["-L", label])? {
        return Ok(PathBuf::from(path));
    }
    let patterns = [
        format!("/dev/disk/by-id/*{disk_id}*"),
        "/dev/vd?".to_owned(),
        "/dev/sd?".to_owned(),
        "/dev/xvd?".to_owned(),
        "/dev/nvme?n?".to_owned(),
        "/dev/pmem?".to_owned(),
    ];
    for pattern in patterns {
        let output = Command::new("sh")
            .arg("-c")
            .arg(format!("for candidate in {pattern}; do [ -e \"$candidate\" ] && printf '%s\n' \"$candidate\"; done"))
            .output()
            .context("failed to enumerate disk candidates")?;
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            if command::output_trimmed("blkid", &["-o", "value", "-s", "LABEL", line])?.as_deref()
                == Some(label)
            {
                return Ok(PathBuf::from(line));
            }
        }
    }
    Err(anyhow!("no btrfs disk with label {label} and id {disk_id}"))
}
