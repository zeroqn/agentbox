//! OMP home bind mount contribution.
//!
//! This file owns only the existing `.omp` mount contribution; it does not
//! define new mount policy or validation behavior.

use anyhow::Result;
use std::fs;
use std::path::Path;

use crate::runtime::launch::config::{BindMount, OMP_TAG, OMP_TARGET};

pub(crate) fn prepare(home_dir: &Path) -> Result<BindMount> {
    let omp_dir = home_dir.join(".omp");
    fs::create_dir_all(&omp_dir)
        .map_err(|err| anyhow::anyhow!("failed to create '{}': {err}", omp_dir.display()))?;
    Ok(super::bind_mount(&omp_dir, OMP_TAG, OMP_TARGET))
}
