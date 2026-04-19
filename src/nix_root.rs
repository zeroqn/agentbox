use anyhow::{anyhow, Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use crate::mounts::format_mount_arg;
use crate::podman::run_podman;
use crate::{
    HOST_NIX_LOG, HOST_NIX_ROOT_DIR, HOST_NIX_STORE, HOST_NIX_VAR, NIX_LOG_DIR, NIX_MARKER_FILE,
    NIX_STORE_DIR, NIX_VAR_DIR, SEED_MOUNT_POINT,
};

#[derive(Debug, Clone)]
pub(super) struct PersistentNixRoot {
    pub(super) store_dir: PathBuf,
    pub(super) var_nix_dir: PathBuf,
    pub(super) log_nix_dir: PathBuf,
    pub(super) marker_file: PathBuf,
}

impl PersistentNixRoot {
    pub(super) fn new(state_root: &Path) -> Self {
        let root = state_root.join(HOST_NIX_ROOT_DIR);
        Self {
            store_dir: root.join(NIX_STORE_DIR),
            var_nix_dir: root.join(NIX_VAR_DIR).join("nix"),
            log_nix_dir: root.join(NIX_VAR_DIR).join(NIX_LOG_DIR).join("nix"),
            marker_file: root.join(NIX_MARKER_FILE),
        }
    }

    pub(super) fn root_dir(&self) -> &Path {
        self.marker_file.parent().unwrap_or_else(|| Path::new("."))
    }

    pub(super) fn mounts(&self) -> [(&Path, &str); 3] {
        [
            (self.store_dir.as_path(), HOST_NIX_STORE),
            (self.var_nix_dir.as_path(), HOST_NIX_VAR),
            (self.log_nix_dir.as_path(), HOST_NIX_LOG),
        ]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum NixRootState {
    Missing,
    Ready,
    Inconsistent,
}

pub(super) fn prepare_persistent_nix_root(
    state_root: &Path,
    image: &str,
) -> Result<PersistentNixRoot> {
    let nix_root = PersistentNixRoot::new(state_root);
    match inspect_persistent_nix_root(&nix_root)? {
        NixRootState::Ready => {
            ensure_persistent_nix_log_dir(&nix_root)?;
            return Ok(nix_root);
        }
        NixRootState::Inconsistent => {
            return Err(anyhow!(
                "'{}' contains partial Nix state without '{}'; remove or repair that state root before retrying",
                nix_root.store_dir.parent().unwrap_or(state_root).display(),
                nix_root
                    .marker_file
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
            ));
        }
        NixRootState::Missing => {}
    }

    seed_persistent_nix_root(&nix_root, image, false)?;
    ensure_persistent_nix_log_dir(&nix_root)?;
    fs::write(
        &nix_root.marker_file,
        format!("seeded-from-image={image}\n"),
    )
    .with_context(|| format!("failed to write '{}'", nix_root.marker_file.display()))?;

    Ok(nix_root)
}

pub(super) fn inspect_persistent_nix_root(nix_root: &PersistentNixRoot) -> Result<NixRootState> {
    let marker_exists = nix_root.marker_file.is_file();
    let store_exists = nix_root.store_dir.is_dir();
    let var_exists = nix_root.var_nix_dir.is_dir();

    if marker_exists {
        return if store_exists && var_exists {
            Ok(NixRootState::Ready)
        } else {
            Ok(NixRootState::Inconsistent)
        };
    }

    let store_empty = dir_is_empty_or_missing(&nix_root.store_dir)?;
    let var_empty = dir_is_empty_or_missing(&nix_root.var_nix_dir)?;
    let log_empty = dir_is_empty_or_missing(&nix_root.log_nix_dir)?;

    if store_empty && var_empty && log_empty {
        Ok(NixRootState::Missing)
    } else {
        Ok(NixRootState::Inconsistent)
    }
}

fn dir_is_empty_or_missing(path: &Path) -> Result<bool> {
    if !path.exists() {
        return Ok(true);
    }
    let mut entries =
        fs::read_dir(path).with_context(|| format!("failed to read '{}'", path.display()))?;
    Ok(entries.next().is_none())
}

fn seed_persistent_nix_root(
    nix_root: &PersistentNixRoot,
    image: &str,
    replace_existing: bool,
) -> Result<()> {
    let root_dir = nix_root.root_dir();
    fs::create_dir_all(root_dir)
        .with_context(|| format!("failed to create '{}'", root_dir.display()))?;

    let host_seed_mount = format_mount_arg(root_dir, SEED_MOUNT_POINT)?;
    let seed_script = build_seed_script(replace_existing);
    let args = build_seed_podman_args(image, &host_seed_mount, &seed_script);

    let status = run_podman(
        args,
        Stdio::null(),
        Stdio::inherit(),
        Stdio::inherit(),
        "failed to seed the external Nix root from the container image",
    )?;
    if !status.success() {
        return Err(anyhow!(
            "seeding '{}' from image '{}' failed",
            root_dir.display(),
            image
        ));
    }

    Ok(())
}

pub(super) fn build_seed_podman_args(
    image: &str,
    host_seed_mount: &str,
    seed_script: &str,
) -> Vec<String> {
    vec![
        "run".to_owned(),
        "--rm".to_owned(),
        "--user".to_owned(),
        "0:0".to_owned(),
        "--volume".to_owned(),
        host_seed_mount.to_owned(),
        image.to_owned(),
        "bash".to_owned(),
        "-lc".to_owned(),
        seed_script.to_owned(),
    ]
}

pub(super) fn build_seed_script(replace_existing: bool) -> String {
    let mut script = String::from("set -euo pipefail\n");
    if replace_existing {
        script.push_str(&format!(
            "find {mount} -mindepth 1 -maxdepth 1 -exec rm -rf -- {{}} +\n",
            mount = SEED_MOUNT_POINT,
        ));
    }
    script.push_str(&format!(
        "mkdir -p {mount}/{store}\nmkdir -p {mount}/{var_dir}/nix\nmkdir -p {mount}/{var_dir}/{log_dir}/nix\ncp -a {nix_store}/. {mount}/{store}/\nif [ -d {nix_var} ]; then\n  cp -a {nix_var}/. {mount}/{var_dir}/nix/\nfi\n",
        mount = SEED_MOUNT_POINT,
        store = NIX_STORE_DIR,
        var_dir = NIX_VAR_DIR,
        log_dir = NIX_LOG_DIR,
        nix_store = HOST_NIX_STORE,
        nix_var = HOST_NIX_VAR,
    ));
    script
}

pub(super) fn ensure_persistent_nix_log_dir(nix_root: &PersistentNixRoot) -> Result<()> {
    fs::create_dir_all(&nix_root.log_nix_dir)
        .with_context(|| format!("failed to create '{}'", nix_root.log_nix_dir.display()))
}

#[cfg(test)]
mod tests;
