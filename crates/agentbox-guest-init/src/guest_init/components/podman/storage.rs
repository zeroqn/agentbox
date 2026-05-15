use anyhow::Result;
use std::path::{Path, PathBuf};

use crate::guest_init::components::env::LibkrunEnv;
use crate::guest_init::components::home::identity::DevIdentity;
use crate::guest_init::components::podman::config::{
    containers_conf, policy_json, registries_conf, storage_conf, PodmanToolPaths,
};
use crate::guest_init::fs;

pub(in crate::guest_init) fn bootstrap(
    identity: &DevIdentity,
    env_contract: &LibkrunEnv,
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
    crate::guest_init::components::disk::containers::ensure_mounted(
        &env_contract.containers_disk_label,
        &env_contract.containers_disk_id,
    )?;

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

#[cfg(test)]
#[path = "storage_tests.rs"]
mod tests;
