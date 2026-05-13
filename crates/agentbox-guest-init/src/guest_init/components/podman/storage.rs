use anyhow::{anyhow, Context, Result};
use std::path::{Path, PathBuf};

use crate::guest_init::command;
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
    let mount = Path::new("/home/dev/.local/share/containers");
    let storage = mount.join("storage");
    let config_dir = Path::new("/home/dev/.config/containers");
    let run_dir = PathBuf::from(format!("/run/user/{}", identity.uid));
    let runroot = run_dir.join("containers");
    fs::create_dir_all(mount)?;
    fs::create_dir_all(config_dir)?;
    fs::create_dir_all(&runroot)?;

    let disk = crate::guest_init::components::disk::btrfs::find_labeled_disk(
        &env_contract.containers_disk_label,
        &env_contract.containers_disk_id,
    )
    .with_context(|| {
        format!(
            "libkrun container storage btrfs disk not found (label={} id={})",
            env_contract.containers_disk_label, env_contract.containers_disk_id
        )
    })?;
    if !command::status_ok("findmnt", &["-rn", path_str(mount)?])? {
        command::run(
            "mount",
            &["-t", "btrfs", path_str(&disk)?, path_str(mount)?],
        )
        .context("failed to mount libkrun container storage btrfs disk")?;
    }
    if let Err(err) = command::run("btrfs", &["filesystem", "resize", "max", path_str(mount)?]) {
        eprintln!(
            "agentbox-guest-init: warning: btrfs resize max failed for '{}': {err:#}; continuing with existing container storage filesystem size",
            mount.display()
        );
    }

    for path in [
        mount,
        storage.as_path(),
        config_dir,
        run_dir.as_path(),
        runroot.as_path(),
    ] {
        fs::create_dir_all(path)?;
        fs::chown(path, identity.uid, identity.gid)?;
    }
    fs::chmod(&run_dir, 0o700)?;
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

fn path_str(path: &Path) -> Result<&str> {
    path.to_str()
        .ok_or_else(|| anyhow!("path is not valid UTF-8: {}", path.display()))
}
