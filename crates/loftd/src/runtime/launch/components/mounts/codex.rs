//! Codex home bind mount contribution.
//!
//! This file owns only the existing `.codex` mount contribution; it does not
//! define new mount policy or validation behavior.

use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

use crate::runtime::launch::config::{BindMount, CODEX_TAG, CODEX_TARGET};

pub(crate) fn prepare(home_dir: &Path) -> Result<BindMount> {
    let codex_dir = home_dir.join(".codex");
    fs::create_dir_all(&codex_dir)
        .map_err(|err| anyhow::anyhow!("failed to create '{}': {err}", codex_dir.display()))?;
    let codex_dir = fs::canonicalize(&codex_dir)
        .with_context(|| format!("failed to inspect mount source '{}'", codex_dir.display()))?;
    Ok(super::bind_mount(&codex_dir, CODEX_TAG, CODEX_TARGET))
}
