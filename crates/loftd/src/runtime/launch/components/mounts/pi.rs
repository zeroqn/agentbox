//! Pi home bind mount contribution.
//!
//! This file owns only the existing `.pi` mount contribution; it does not define
//! new mount policy or validation behavior.

use anyhow::Result;
use std::fs;
use std::path::Path;

use crate::runtime::launch::config::{BindMount, PI_TAG, PI_TARGET};

pub(crate) fn prepare(home_dir: &Path) -> Result<BindMount> {
    let pi_dir = home_dir.join(".pi");
    fs::create_dir_all(&pi_dir)
        .map_err(|err| anyhow::anyhow!("failed to create '{}': {err}", pi_dir.display()))?;
    Ok(super::bind_mount(&pi_dir, PI_TAG, PI_TARGET))
}
