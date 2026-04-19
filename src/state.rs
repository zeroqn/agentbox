use anyhow::{anyhow, Context, Result};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use crate::derive_workspace_slug;

const APP_DIR_NAME: &str = "agentbox";
const CONFIG_FILE_NAME: &str = "agentbox.toml";
const STATE_CONFIG_SECTION: &str = "state";
const STATE_LOCATION_KEY: &str = "location";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct StateLayout {
    pub(super) root_dir: PathBuf,
}

impl StateLayout {
    pub(super) fn new(root_dir: PathBuf) -> Self {
        Self { root_dir }
    }
}

pub(super) fn resolve_state_layout(cwd: &Path) -> Result<StateLayout> {
    let xdg_state_home = env::var_os("XDG_STATE_HOME").map(PathBuf::from);
    let xdg_config_home = env::var_os("XDG_CONFIG_HOME").map(PathBuf::from);
    let home_dir = env::var_os("HOME").map(PathBuf::from);

    resolve_state_layout_from_env(
        cwd,
        xdg_state_home.as_deref(),
        xdg_config_home.as_deref(),
        home_dir.as_deref(),
    )
}

pub(super) fn resolve_state_layout_from_env(
    cwd: &Path,
    xdg_state_home: Option<&Path>,
    xdg_config_home: Option<&Path>,
    home_dir: Option<&Path>,
) -> Result<StateLayout> {
    let default_location_root = default_state_location_root(xdg_state_home, home_dir)?;
    let config_path = default_config_path(xdg_config_home, home_dir)?;
    let location_root =
        read_state_location_override(&config_path)?.unwrap_or(default_location_root);

    Ok(StateLayout::new(
        location_root
            .join(APP_DIR_NAME)
            .join(derive_workspace_slug(cwd)),
    ))
}

pub(super) fn default_state_location_root(
    xdg_state_home: Option<&Path>,
    home_dir: Option<&Path>,
) -> Result<PathBuf> {
    if let Some(path) = xdg_state_home {
        return Ok(path.to_path_buf());
    }

    let home_dir =
        home_dir.ok_or_else(|| anyhow!("HOME is not set and XDG_STATE_HOME is not available"))?;
    Ok(home_dir.join(".local").join("state"))
}

pub(super) fn default_config_path(
    xdg_config_home: Option<&Path>,
    home_dir: Option<&Path>,
) -> Result<PathBuf> {
    if let Some(path) = xdg_config_home {
        return Ok(path.join(APP_DIR_NAME).join(CONFIG_FILE_NAME));
    }

    let home_dir =
        home_dir.ok_or_else(|| anyhow!("HOME is not set and XDG_CONFIG_HOME is not available"))?;
    Ok(home_dir
        .join(".config")
        .join(APP_DIR_NAME)
        .join(CONFIG_FILE_NAME))
}

fn read_state_location_override(config_path: &Path) -> Result<Option<PathBuf>> {
    if !config_path.exists() {
        return Ok(None);
    }

    let contents = fs::read_to_string(config_path)
        .with_context(|| format!("failed to read '{}'", config_path.display()))?;
    parse_state_location_override(&contents).with_context(|| {
        format!(
            "failed to parse state location from '{}'",
            config_path.display()
        )
    })
}

pub(super) fn parse_state_location_override(contents: &str) -> Result<Option<PathBuf>> {
    let mut in_state_section = false;

    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';') {
            continue;
        }

        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_state_section = trimmed[1..trimmed.len() - 1].trim() == STATE_CONFIG_SECTION;
            continue;
        }

        if !in_state_section {
            continue;
        }

        let Some((key, value)) = trimmed.split_once('=') else {
            continue;
        };

        if key.trim() != STATE_LOCATION_KEY {
            continue;
        }

        let value = value.trim();
        if value.len() < 2 || !value.starts_with('"') || !value.ends_with('"') {
            return Err(anyhow!(
                "[state].location must be a double-quoted absolute path"
            ));
        }

        let path = PathBuf::from(&value[1..value.len() - 1]);
        if !path.is_absolute() {
            return Err(anyhow!("[state].location must be an absolute path"));
        }

        return Ok(Some(path));
    }

    Ok(None)
}

#[cfg(test)]
mod tests;
