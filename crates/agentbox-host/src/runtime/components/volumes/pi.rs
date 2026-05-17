use anyhow::{Context, Result};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use crate::CONTAINER_PI_DIR;
use crate::podman::volume::format_mount_arg;

pub fn prepare() -> Result<String> {
    let home_dir = env::var_os("HOME").context("HOME is not set; cannot locate '~/.pi'")?;
    prepare_at(&PathBuf::from(home_dir))
}

pub fn prepare_at(home_dir: &Path) -> Result<String> {
    let pi_dir = home_dir.join(".pi");
    fs::create_dir_all(&pi_dir)
        .with_context(|| format!("failed to create '{}'", pi_dir.display()))?;
    format_mount_arg(&pi_dir, CONTAINER_PI_DIR)
}
