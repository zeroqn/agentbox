use anyhow::{Context, Result};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use crate::{CONTAINER_CARGO_DIR, CONTAINER_CODEX_DIR, HOST_OVERLAY_DIR};

pub(crate) fn prepare_host_codex_mount() -> Result<String> {
    let home_dir = env::var_os("HOME").context("HOME is not set; cannot locate '~/.codex'")?;
    prepare_host_codex_mount_at(&PathBuf::from(home_dir))
}

pub(crate) fn prepare_host_codex_mount_at(home_dir: &Path) -> Result<String> {
    let codex_dir = home_dir.join(".codex");
    fs::create_dir_all(&codex_dir)
        .with_context(|| format!("failed to create '{}'", codex_dir.display()))?;
    format_mount_arg(&codex_dir, CONTAINER_CODEX_DIR)
}

pub(crate) fn prepare_project_cargo_mount(cwd: &Path) -> Result<String> {
    let cargo_dir = cwd.join(HOST_OVERLAY_DIR).join("cargo");
    fs::create_dir_all(&cargo_dir)
        .with_context(|| format!("failed to create '{}'", cargo_dir.display()))?;
    format_mount_arg(&cargo_dir, CONTAINER_CARGO_DIR)
}

pub(crate) fn format_mount_arg(path: &Path, destination: &str) -> Result<String> {
    format_mount_arg_with_options(path, destination, None)
}

pub(crate) fn format_mount_arg_with_options(
    path: &Path,
    destination: &str,
    options: Option<&str>,
) -> Result<String> {
    let path = path.to_str().with_context(|| {
        format!(
            "path '{}' is not valid UTF-8 and cannot be mounted",
            path.display()
        )
    })?;

    let mut mount = format!("{path}:{destination}");
    if let Some(options) = options {
        if !options.is_empty() {
            mount.push(':');
            mount.push_str(options);
        }
    }

    Ok(mount)
}
