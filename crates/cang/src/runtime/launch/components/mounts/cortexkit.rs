//! Cortexkit home bind mount contribution.
//!
//! This file owns only the `.local/share/cortexkit` mount contribution; it does
//! not define new mount policy or validation behavior.

use anyhow::Result;
use std::fs;
use std::path::Path;

use crate::runtime::launch::components::mounts::resolve_dir;

use crate::runtime::launch::config::{BindMount, CORTEXKIT_TAG, CORTEXKIT_TARGET};

pub(crate) fn prepare(home_dir: &Path) -> Result<BindMount> {
    let cortexkit_dir = home_dir.join(".local/share/cortexkit");
    fs::create_dir_all(&cortexkit_dir)
        .map_err(|err| anyhow::anyhow!("failed to create '{}': {err}", cortexkit_dir.display()))?;
    let cortexkit_dir = resolve_dir(&cortexkit_dir)?;
    Ok(super::bind_mount(
        &cortexkit_dir,
        CORTEXKIT_TAG,
        CORTEXKIT_TARGET,
    ))
}
