use anyhow::{Context, Result};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use crate::CONTAINER_CODEX_DIR;
use crate::podman::volume::format_mount_arg;

pub fn prepare() -> Result<String> {
    let home_dir = env::var_os("HOME").context("HOME is not set; cannot locate '~/.codex'")?;
    prepare_at(&PathBuf::from(home_dir))
}

pub fn prepare_at(home_dir: &Path) -> Result<String> {
    let codex_dir = home_dir.join(".codex");
    fs::create_dir_all(&codex_dir)
        .with_context(|| format!("failed to create '{}'", codex_dir.display()))?;
    format_mount_arg(&codex_dir, CONTAINER_CODEX_DIR)
}
