use anyhow::{Context, Result, anyhow};
use std::path::PathBuf;
use std::process::Command;

use crate::guest_init::command;

// Keep the original no-preference entrypoint available for any future callers
// that need generic label/id discovery without role-specific device hints.
#[allow(dead_code)]
pub(in crate::guest_init) fn find_labeled_disk(label: &str, disk_id: &str) -> Result<PathBuf> {
    find_labeled_disk_with_preferred(label, disk_id, &[])
}

pub(in crate::guest_init::components::disk) fn find_labeled_disk_with_preferred(
    label: &str,
    disk_id: &str,
    preferred_candidates: &[&str],
) -> Result<PathBuf> {
    find_labeled_disk_with_probe(label, disk_id, preferred_candidates, &SystemDiskProbe)
}

fn find_labeled_disk_with_probe(
    label: &str,
    disk_id: &str,
    preferred_candidates: &[&str],
    probe: &impl DiskProbe,
) -> Result<PathBuf> {
    for candidate in preferred_candidates {
        if probe.candidate_label(candidate)?.as_deref() == Some(label) {
            return Ok(PathBuf::from(candidate));
        }
    }

    if let Some(path) = probe.lookup_label(label)? {
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
        for candidate in probe.enumerate_candidates(&pattern)? {
            if probe.candidate_label(&candidate)?.as_deref() == Some(label) {
                return Ok(PathBuf::from(candidate));
            }
        }
    }
    Err(anyhow!("no btrfs disk with label {label} and id {disk_id}"))
}

trait DiskProbe {
    fn lookup_label(&self, label: &str) -> Result<Option<String>>;
    fn candidate_label(&self, candidate: &str) -> Result<Option<String>>;
    fn enumerate_candidates(&self, pattern: &str) -> Result<Vec<String>>;
}

struct SystemDiskProbe;

impl DiskProbe for SystemDiskProbe {
    fn lookup_label(&self, label: &str) -> Result<Option<String>> {
        command::output_trimmed("blkid", &["-L", label])
    }

    fn candidate_label(&self, candidate: &str) -> Result<Option<String>> {
        command::output_trimmed("blkid", &["-o", "value", "-s", "LABEL", candidate])
    }

    fn enumerate_candidates(&self, pattern: &str) -> Result<Vec<String>> {
        let output = Command::new("sh")
            .arg("-c")
            .arg(format!(
                "for candidate in {pattern}; do [ -e \"$candidate\" ] && printf '%s\n' \"$candidate\"; done"
            ))
            .output()
            .context("failed to enumerate disk candidates")?;
        Ok(String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(str::to_owned)
            .collect())
    }
}

#[cfg(test)]
#[path = "btrfs_tests.rs"]
mod tests;
