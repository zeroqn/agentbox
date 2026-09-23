//! Sccache bind mount contribution.
//!
//! This file owns only the existing sccache cache mount contribution; it does
//! not define new mount policy or validation behavior.

use anyhow::Result;
use std::fs;
use std::os::unix::fs::PermissionsExt;

use crate::runtime::launch::components::mounts::resolve_dir;

use crate::runtime::launch::config::{BindMount, SCCACHE_TAG, SCCACHE_TARGET};
use crate::state::StateLayout;

pub(crate) fn prepare(state_layout: &StateLayout) -> Result<BindMount> {
    let sccache_dir = state_layout.sccache_dir();
    fs::create_dir_all(&sccache_dir)
        .map_err(|err| anyhow::anyhow!("failed to create '{}': {err}", sccache_dir.display()))?;
    fs::set_permissions(&sccache_dir, fs::Permissions::from_mode(0o700))
        .map_err(|err| anyhow::anyhow!("failed to chmod 700 '{}': {err}", sccache_dir.display()))?;
    let sccache_dir = resolve_dir(&sccache_dir)?;
    Ok(super::bind_mount(&sccache_dir, SCCACHE_TAG, SCCACHE_TARGET))
}
