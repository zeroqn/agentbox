use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};

use crate::guest_init::command;
use crate::guest_init::components::env::{ContainerStoreBackend, LoftdEnv};
use crate::guest_init::components::home::identity::DevIdentity;
use crate::guest_init::components::podman::config::{
    PodmanToolPaths, containers_conf, policy_json, registries_conf, storage_conf,
};
use crate::guest_init::fs;

pub(in crate::guest_init) fn bootstrap(
    identity: &DevIdentity,
    env_contract: &LoftdEnv,
    tool_paths: &PodmanToolPaths,
) -> Result<()> {
    let mount = Path::new(crate::guest_init::components::disk::containers::MOUNT_POINT);
    let storage = mount.join("storage");
    let config_dir = Path::new("/home/dev/.config/containers");
    let run_dir = PathBuf::from(format!("/run/user/{}", identity.uid));
    let runroot = run_dir.join("containers");
    fs::create_dir_all(config_dir)?;
    crate::guest_init::components::rootless::runtime_dir::ensure_user_runtime_dir(identity)?;
    fs::create_dir_all(&runroot)?;
    ensure_container_store_available(env_contract, mount)?;

    for path in [mount, storage.as_path(), config_dir, runroot.as_path()] {
        fs::create_dir_all(path)?;
        fs::chown(path, identity.uid, identity.gid)?;
    }
    fs::write_file(
        &config_dir.join("storage.conf"),
        &storage_conf(identity),
        0o644,
    )?;
    fs::write_file(
        &config_dir.join("containers.conf"),
        &containers_conf(tool_paths),
        0o644,
    )?;
    fs::write_file(
        &config_dir.join("registries.conf"),
        registries_conf(),
        0o644,
    )?;
    fs::write_file(&config_dir.join("policy.json"), policy_json(), 0o644)?;
    for file in [
        "storage.conf",
        "containers.conf",
        "registries.conf",
        "policy.json",
    ] {
        fs::chown(&config_dir.join(file), identity.uid, identity.gid)?;
    }
    Ok(())
}

fn ensure_container_store_available(env_contract: &LoftdEnv, mount: &Path) -> Result<()> {
    match env_contract.container_store_backend {
        ContainerStoreBackend::Bind => ensure_bind_store_is_btrfs(mount),
        ContainerStoreBackend::RawDisk => {
            crate::guest_init::components::disk::containers::ensure_mounted(
                &env_contract.containers_disk_label,
                &env_contract.containers_disk_id,
            )?;
            Ok(())
        }
    }
}

fn ensure_bind_store_is_btrfs(mount: &Path) -> Result<()> {
    let mount_text = mount
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("path is not valid UTF-8: {}", mount.display()))?;
    let fstype = command::output_trimmed("findmnt", &["-no", "FSTYPE", "-T", mount_text])
        .context("failed to inspect bind container store filesystem")?;
    validate_bind_store_fstype(fstype.as_deref()).with_context(|| {
        format!(
            "bind container store '{}' requires a btrfs-backed host state directory; rerun with --container-store raw-disk to use loftd's raw btrfs disk fallback",
            mount.display()
        )
    })
}

fn validate_bind_store_fstype(fstype: Option<&str>) -> Result<()> {
    match fstype {
        Some("btrfs") => Ok(()),
        Some(other) => bail!("container store filesystem is '{other}', not 'btrfs'"),
        None => bail!("container store filesystem could not be detected"),
    }
}

#[cfg(test)]
#[path = "storage_tests.rs"]
mod tests;
