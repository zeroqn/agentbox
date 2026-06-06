//! Bind mount contribution validation and workspace compatibility ownership.

use anyhow::{Result, anyhow};
use std::collections::BTreeSet;
use std::path::Path;

use super::super::model::{BindMount, WORKSPACE_TARGET};

pub(crate) fn workspace_mount(mounts: &[BindMount]) -> Result<&BindMount> {
    mounts
        .iter()
        .find(|mount| mount.target == WORKSPACE_TARGET)
        .ok_or_else(|| anyhow!("loftd launch config requires a {WORKSPACE_TARGET} mount"))
}

pub fn validate_mounts(mounts: &[BindMount]) -> Result<()> {
    if mounts.is_empty() {
        anyhow::bail!("loftd launch config requires at least one bind mount");
    }
    let mut tags = BTreeSet::new();
    let mut targets = BTreeSet::new();
    for mount in mounts {
        if mount.tag.trim().is_empty() {
            anyhow::bail!("loftd bind mount tag cannot be empty");
        }
        if !Path::new(&mount.target).is_absolute() {
            anyhow::bail!(
                "loftd bind mount target '{}' must be absolute",
                mount.target
            );
        }
        if mount.target.contains(".config/codex")
            || mount.source.to_string_lossy().contains(".config/codex")
        {
            anyhow::bail!("loftd bind mounts must not include .config/codex");
        }
        if !tags.insert(mount.tag.as_str()) {
            anyhow::bail!("loftd bind mount tag '{}' is duplicated", mount.tag);
        }
        if !targets.insert(mount.target.as_str()) {
            anyhow::bail!("loftd bind mount target '{}' is duplicated", mount.target);
        }
    }
    workspace_mount(mounts)?;
    Ok(())
}
