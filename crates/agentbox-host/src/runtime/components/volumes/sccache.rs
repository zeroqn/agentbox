use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

use crate::CONTAINER_SCCACHE_DIR;
use crate::podman::volume::format_mount_arg;

pub fn prepare(sccache_dir: &Path) -> Result<String> {
    fs::create_dir_all(sccache_dir)
        .with_context(|| format!("failed to create '{}'", sccache_dir.display()))?;
    format_mount_arg(sccache_dir, CONTAINER_SCCACHE_DIR)
}
