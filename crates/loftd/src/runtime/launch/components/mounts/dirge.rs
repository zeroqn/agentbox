//! Dirge home bind mount contributions.
//!
//! This file owns only Dirge config/state mount contributions; it does not
//! define new mount policy or validation behavior.

use anyhow::Result;
use std::fs;
use std::path::Path;

use crate::runtime::launch::components::mounts::resolve_dir;

use crate::runtime::launch::config::{
    BindMount, DIRGE_CONFIG_TAG, DIRGE_CONFIG_TARGET, DIRGE_DATA_TAG, DIRGE_DATA_TARGET,
    DIRGE_HOME_TAG, DIRGE_HOME_TARGET,
};

pub(crate) fn prepare(home_dir: &Path) -> Result<Vec<BindMount>> {
    Ok(vec![
        prepare_dir(
            home_dir,
            ".config/dirge",
            DIRGE_CONFIG_TAG,
            DIRGE_CONFIG_TARGET,
        )?,
        prepare_dir(
            home_dir,
            ".local/share/dirge",
            DIRGE_DATA_TAG,
            DIRGE_DATA_TARGET,
        )?,
        prepare_dir(home_dir, ".dirge", DIRGE_HOME_TAG, DIRGE_HOME_TARGET)?,
    ])
}

fn prepare_dir(home_dir: &Path, relative_path: &str, tag: &str, target: &str) -> Result<BindMount> {
    let dirge_dir = home_dir.join(relative_path);
    fs::create_dir_all(&dirge_dir)
        .map_err(|err| anyhow::anyhow!("failed to create '{}': {err}", dirge_dir.display()))?;
    let dirge_dir = resolve_dir(&dirge_dir)?;
    Ok(super::bind_mount(&dirge_dir, tag, target))
}
