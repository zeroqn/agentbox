use anyhow::{anyhow, Result};
use std::env;
use std::path::{Path, PathBuf};

use crate::config;
use crate::naming::derive_workspace_slug;

const APP_DIR_NAME: &str = "agentbox";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateLayout {
    app_dir: PathBuf,
    root_dir: PathBuf,
}

impl StateLayout {
    pub fn root_dir(&self) -> &Path {
        &self.root_dir
    }

    pub fn sccache_dir(&self) -> PathBuf {
        self.app_dir.join("sccache")
    }

    fn new(app_dir: PathBuf, root_dir: PathBuf) -> Self {
        Self { app_dir, root_dir }
    }
}

pub fn resolve_state_layout(cwd: &Path) -> Result<StateLayout> {
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

fn resolve_state_layout_from_env(
    cwd: &Path,
    xdg_state_home: Option<&Path>,
    xdg_config_home: Option<&Path>,
    home_dir: Option<&Path>,
) -> Result<StateLayout> {
    let default_location_root = default_state_location_root(xdg_state_home, home_dir)?;
    let location_root = config::state::read_state_location_override(xdg_config_home, home_dir)?
        .unwrap_or(default_location_root);

    let app_dir = location_root.join(APP_DIR_NAME);
    Ok(StateLayout::new(
        app_dir.clone(),
        app_dir.join(derive_workspace_slug(cwd)),
    ))
}

fn default_state_location_root(
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

#[cfg(test)]
mod tests;
