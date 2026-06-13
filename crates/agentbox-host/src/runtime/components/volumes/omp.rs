use anyhow::{Context, Result};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use crate::CONTAINER_OMP_DIR;
use crate::podman::volume::format_mount_arg;

pub fn prepare() -> Result<String> {
    let home_dir = env::var_os("HOME").context("HOME is not set; cannot locate '~/.omp'")?;
    prepare_at(&PathBuf::from(home_dir))
}

pub fn prepare_at(home_dir: &Path) -> Result<String> {
    let omp_dir = home_dir.join(".omp");
    fs::create_dir_all(&omp_dir)
        .with_context(|| format!("failed to create '{}'", omp_dir.display()))?;
    format_mount_arg(&omp_dir, CONTAINER_OMP_DIR)
}
