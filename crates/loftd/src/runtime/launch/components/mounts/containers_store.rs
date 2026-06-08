//! Container store bind mount contribution.
//!
//! This file owns only the host state directory carrier for bind-mode nested
//! Podman storage. Backend policy remains host launch-plan state.

use anyhow::Result;
use std::fs;
use std::os::unix::fs::PermissionsExt;

use crate::runtime::launch::config::{BindMount, CONTAINERS_STORE_TAG, CONTAINERS_STORE_TARGET};
use crate::state::StateLayout;

pub(crate) fn prepare(state_layout: &StateLayout) -> Result<BindMount> {
    let store_dir = state_layout.root_dir().join("containers");
    fs::create_dir_all(&store_dir)
        .map_err(|err| anyhow::anyhow!("failed to create '{}': {err}", store_dir.display()))?;
    fs::set_permissions(&store_dir, fs::Permissions::from_mode(0o700))
        .map_err(|err| anyhow::anyhow!("failed to chmod 700 '{}': {err}", store_dir.display()))?;
    Ok(super::bind_mount(
        &store_dir,
        CONTAINERS_STORE_TAG,
        CONTAINERS_STORE_TARGET,
    ))
}
