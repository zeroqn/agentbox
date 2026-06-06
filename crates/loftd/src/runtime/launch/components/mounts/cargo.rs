//! Cargo cache bind mount contribution.
//!
//! This file owns only the existing cargo cache mount contribution; it does not
//! define new mount policy or validation behavior.

use anyhow::Result;
use std::fs;

use crate::runtime::launch::config::{BindMount, CARGO_TAG, CARGO_TARGET};
use crate::state::StateLayout;

pub(crate) fn prepare(state_layout: &StateLayout) -> Result<BindMount> {
    let cargo_dir = state_layout.root_dir().join("cargo");
    fs::create_dir_all(&cargo_dir)
        .map_err(|err| anyhow::anyhow!("failed to create '{}': {err}", cargo_dir.display()))?;
    Ok(super::bind_mount(&cargo_dir, CARGO_TAG, CARGO_TARGET))
}
