use anyhow::{Context, Result};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use self::format::format_mount_arg;
use crate::{CONTAINER_CARGO_DIR, CONTAINER_CODEX_DIR};

pub mod format;

pub fn prepare_host_codex_mount() -> Result<String> {
    let home_dir = env::var_os("HOME").context("HOME is not set; cannot locate '~/.codex'")?;
    prepare_host_codex_mount_at(&PathBuf::from(home_dir))
}

pub fn prepare_project_cargo_mount(state_root: &Path) -> Result<String> {
    let cargo_dir = state_root.join("cargo");
    fs::create_dir_all(&cargo_dir)
        .with_context(|| format!("failed to create '{}'", cargo_dir.display()))?;
    format_mount_arg(&cargo_dir, CONTAINER_CARGO_DIR)
}

fn prepare_host_codex_mount_at(home_dir: &Path) -> Result<String> {
    let codex_dir = home_dir.join(".codex");
    fs::create_dir_all(&codex_dir)
        .with_context(|| format!("failed to create '{}'", codex_dir.display()))?;
    format_mount_arg(&codex_dir, CONTAINER_CODEX_DIR)
}

#[cfg(test)]
mod tests;
