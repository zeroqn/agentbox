use anyhow::{Context, Result};
use std::path::Path;

use crate::guest_init::components::home::identity::DevIdentity;
use crate::guest_init::fs;

pub(in crate::guest_init) fn materialize_config(
    identity: &DevIdentity,
    source: &Path,
    config_dir: &Path,
    set_ownership: bool,
) -> Result<()> {
    create_dir_for_identity(
        &identity.home.join(".cache/starship"),
        identity,
        set_ownership,
    )?;
    copy_config_file_if_missing(
        source,
        &config_dir.join("starship.toml"),
        identity,
        set_ownership,
    )
    .context("failed to materialize bundled starship config")
}

fn create_dir_for_identity(path: &Path, identity: &DevIdentity, set_ownership: bool) -> Result<()> {
    fs::create_dir_all(path)?;
    fs::chmod(path, 0o700)?;
    if set_ownership {
        fs::chown(path, identity.uid, identity.gid)?;
    }
    Ok(())
}

fn copy_config_file_if_missing(
    source: &Path,
    target: &Path,
    identity: &DevIdentity,
    set_ownership: bool,
) -> Result<()> {
    if target.exists() {
        return Ok(());
    }
    if let Some(parent) = target.parent() {
        create_dir_for_identity(parent, identity, set_ownership)?;
    }
    std::fs::copy(source, target).with_context(|| {
        format!(
            "failed to copy {} to {}",
            source.display(),
            target.display()
        )
    })?;
    fs::chmod(target, 0o644)?;
    if set_ownership {
        fs::chown(target, identity.uid, identity.gid)?;
    }
    Ok(())
}
