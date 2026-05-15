use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

use crate::podman::volume::format_mount_arg;
use crate::CONTAINER_CARGO_DIR;

pub fn prepare(state_root: &Path) -> Result<String> {
    let cargo_dir = state_root.join("cargo");
    fs::create_dir_all(&cargo_dir)
        .with_context(|| format!("failed to create '{}'", cargo_dir.display()))?;
    format_mount_arg(&cargo_dir, CONTAINER_CARGO_DIR)
}
