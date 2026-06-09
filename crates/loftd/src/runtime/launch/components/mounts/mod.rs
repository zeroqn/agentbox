//! Development bind mount contributors.
//!
//! Each owner file contributes one existing bind mount. The aggregate preserves
//! order and validation; owner files do not introduce new policy.

use anyhow::Result;
use std::path::Path;

use crate::runtime::launch::config::{BindMount, validate_mounts};
use crate::state::StateLayout;

mod cargo;
mod codex;
pub(crate) mod containers_store;
mod pi;
mod sccache;
mod workspace;

pub(crate) fn prepare_dev_mounts(
    workspace_dir: &Path,
    home_dir: &Path,
    state_layout: &StateLayout,
) -> Result<Vec<BindMount>> {
    let mounts = vec![
        workspace::bind_mount(workspace_dir),
        codex::prepare(home_dir)?,
        pi::prepare(home_dir)?,
        cargo::prepare(state_layout)?,
        sccache::prepare(state_layout)?,
    ];
    validate_mounts(&mounts)?;
    Ok(mounts)
}

fn bind_mount(source: &Path, tag: &str, target: &str) -> BindMount {
    BindMount::directory(source, tag, target)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    use super::*;
    use crate::runtime::launch::config::{
        CARGO_TAG, CARGO_TARGET, CODEX_TAG, CODEX_TARGET, PI_TAG, PI_TARGET, SCCACHE_TAG,
        SCCACHE_TARGET, WORKSPACE_TAG, WORKSPACE_TARGET,
    };
    use crate::state;

    #[test]
    fn dev_mounts_preserve_targets_tags_and_sources() {
        let dir = tempfile::tempdir().expect("tempdir should exist");
        let workspace = dir.path().join("project");
        let home = dir.path().join("home");
        fs::create_dir_all(&workspace).expect("workspace should exist");
        let state_layout = state::resolve_state_layout_from_parts(
            &workspace,
            Some(dir.path().join("state").as_path()),
            Some(home.as_path()),
            None,
        )
        .expect("state layout should resolve");

        let mounts =
            prepare_dev_mounts(&workspace, &home, &state_layout).expect("mounts should prepare");

        assert_eq!(mounts.len(), 5);
        assert_mount(&mounts[0], &workspace, WORKSPACE_TAG, WORKSPACE_TARGET);
        assert_mount(&mounts[1], &home.join(".codex"), CODEX_TAG, CODEX_TARGET);
        assert_mount(&mounts[2], &home.join(".pi"), PI_TAG, PI_TARGET);
        assert_mount(
            &mounts[3],
            &state_layout.root_dir().join("cargo"),
            CARGO_TAG,
            CARGO_TARGET,
        );
        assert_mount(
            &mounts[4],
            &state_layout.sccache_dir(),
            SCCACHE_TAG,
            SCCACHE_TARGET,
        );
        assert!(home.join(".codex").is_dir());
        assert!(home.join(".pi").is_dir());
        assert!(state_layout.root_dir().join("cargo").is_dir());
        assert_eq!(
            fs::metadata(state_layout.sccache_dir())
                .expect("sccache metadata")
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
    }

    #[test]
    fn container_store_mount_uses_state_root_carrier_with_private_permissions() {
        let dir = tempfile::tempdir().expect("tempdir should exist");
        let workspace = dir.path().join("project");
        let home = dir.path().join("home");
        fs::create_dir_all(&workspace).expect("workspace should exist");
        let state_layout = state::resolve_state_layout_from_parts(
            &workspace,
            Some(dir.path().join("state").as_path()),
            Some(home.as_path()),
            None,
        )
        .expect("state layout should resolve");

        let mount = containers_store::prepare(&state_layout).expect("mount should prepare");

        assert_mount(
            &mount,
            &state_layout.root_dir().join("containers"),
            crate::runtime::launch::config::CONTAINERS_STORE_TAG,
            crate::runtime::launch::config::CONTAINERS_STORE_TARGET,
        );
        assert_eq!(
            fs::metadata(state_layout.root_dir().join("containers"))
                .expect("container store metadata")
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
    }

    fn assert_mount(mount: &BindMount, source: &Path, tag: &str, target: &str) {
        assert_eq!(mount.source, source);
        assert_eq!(mount.tag, tag);
        assert_eq!(mount.target, target);
    }
}
