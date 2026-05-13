use anyhow::{anyhow, bail, Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use crate::guest_init::command;
use crate::guest_init::runtime::libkrun::LibkrunEnv;
use crate::guest_init::{fs, process};

pub(in crate::guest_init) const SOCKET_PATH: &str = "/nix/var/nix/daemon-socket/socket";

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::guest_init) enum NixOperation {
    FindDisk,
    BindLower,
    RemountLowerReadOnly,
    MountDisk,
    ResizeDisk,
    PreseedUpper,
    MountOverlay,
    StartDaemon,
    WaitSocket,
}

#[cfg(test)]
pub(in crate::guest_init) fn planned_operations() -> Vec<NixOperation> {
    vec![
        NixOperation::FindDisk,
        NixOperation::BindLower,
        NixOperation::RemountLowerReadOnly,
        NixOperation::MountDisk,
        NixOperation::ResizeDisk,
        NixOperation::PreseedUpper,
        NixOperation::MountOverlay,
        NixOperation::StartDaemon,
        NixOperation::WaitSocket,
    ]
}

pub(in crate::guest_init) fn bootstrap(env_contract: &LibkrunEnv) -> Result<()> {
    if !env_contract.nix_overlay {
        return Ok(());
    }
    if !process::is_root() {
        bail!("libkrun /nix overlay bootstrap must run as root");
    }
    for tool in ["blkid", "mount", "findmnt", "btrfs", "nix-daemon"] {
        command::require_on_path(tool)?;
    }

    let disk = find_btrfs_disk(&env_contract.nix_disk_label, &env_contract.nix_disk_id)
        .with_context(|| {
            format!(
                "libkrun /nix btrfs disk not found (label={} id={})",
                env_contract.nix_disk_label, env_contract.nix_disk_id
            )
        })?;
    let run_dir = Path::new("/run/agentbox");
    let disk_mount = run_dir.join("nix-disk");
    let lower_dir = run_dir.join("nix-lower");
    let upper_dir = disk_mount.join("upper");
    let work_dir = disk_mount.join("work");

    fs::create_dir_all(run_dir)?;
    fs::create_dir_all(&disk_mount)?;
    fs::create_dir_all(&lower_dir)?;

    if !findmnt(&lower_dir)? {
        command::run("mount", &["--bind", "/nix", path_str(&lower_dir)?])
            .context("failed to preserve image /nix lowerdir")?;
        command::run("mount", &["-o", "remount,bind,ro", path_str(&lower_dir)?])
            .context("failed to make image /nix lowerdir read-only")?;
    }

    if !findmnt(&disk_mount)? {
        command::run(
            "mount",
            &["-t", "btrfs", path_str(&disk)?, path_str(&disk_mount)?],
        )
        .context("failed to mount libkrun /nix btrfs disk")?;
    }

    if let Err(err) = command::run(
        "btrfs",
        &["filesystem", "resize", "max", path_str(&disk_mount)?],
    ) {
        eprintln!(
            "agentbox-guest-init: warning: btrfs resize max failed for '{}': {err:#}; continuing with existing filesystem size",
            disk_mount.display()
        );
    }

    preseed_upper(&lower_dir, &upper_dir, &work_dir)?;
    let options = format!(
        "lowerdir={},upperdir={},workdir={}",
        lower_dir.display(),
        upper_dir.display(),
        work_dir.display()
    );
    command::run(
        "mount",
        &["-t", "overlay", "overlay", "-o", &options, "/nix"],
    )
    .context("failed to mount libkrun overlay at /nix")?;

    fs::create_dir_all(Path::new("/nix/var/nix/daemon-socket"))?;
    let mut child = command::spawn_background("nix-daemon", &["--daemon"])
        .context("failed to start nix-daemon")?;
    for _ in 0..100 {
        if Path::new(SOCKET_PATH).exists() {
            std::env::set_var("NIX_REMOTE", format!("unix://{SOCKET_PATH}"));
            return Ok(());
        }
        if let Some(status) = child.try_wait()? {
            bail!("nix-daemon exited before creating '{SOCKET_PATH}' with status {status}");
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    bail!("nix-daemon did not create '{SOCKET_PATH}' before timeout")
}

pub(in crate::guest_init) fn find_btrfs_disk(label: &str, disk_id: &str) -> Result<PathBuf> {
    if let Some(path) = command::output_trimmed("blkid", &["-L", label])? {
        return Ok(PathBuf::from(path));
    }
    let patterns = [
        format!("/dev/disk/by-id/*{disk_id}*"),
        "/dev/vd?".to_owned(),
        "/dev/sd?".to_owned(),
        "/dev/xvd?".to_owned(),
        "/dev/nvme?n?".to_owned(),
        "/dev/pmem?".to_owned(),
    ];
    for pattern in patterns {
        let output = Command::new("sh")
            .arg("-c")
            .arg(format!("for candidate in {pattern}; do [ -e \"$candidate\" ] && printf '%s\n' \"$candidate\"; done"))
            .output()
            .context("failed to enumerate disk candidates")?;
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            if command::output_trimmed("blkid", &["-o", "value", "-s", "LABEL", line])?.as_deref()
                == Some(label)
            {
                return Ok(PathBuf::from(line));
            }
        }
    }
    Err(anyhow!("no btrfs disk with label {label} and id {disk_id}"))
}

fn preseed_upper(lower_dir: &Path, upper_dir: &Path, work_dir: &Path) -> Result<()> {
    fs::create_dir_all(upper_dir)?;
    fs::create_dir_all(work_dir)?;
    fs::create_dir_all(&upper_dir.join("store"))?;
    fs::create_dir_all(&upper_dir.join("var"))?;
    if lower_dir.join("var").is_dir() {
        let source = format!("{}/.", lower_dir.join("var").display());
        command::run(
            "cp",
            &[
                "-a",
                "--no-clobber",
                &source,
                path_str(&upper_dir.join("var"))?,
            ],
        )
        .context("failed to preseed libkrun upperdir /nix/var from image lowerdir")?;
    }
    fs::create_dir_all(&upper_dir.join("var/nix"))?;
    command::run("chown", &[":nixbld", path_str(&upper_dir.join("store"))?])?;
    fs::chmod(&upper_dir.join("store"), 0o1775)?;
    fs::chmod(&upper_dir.join("var"), 0o755)?;
    fs::chmod(&upper_dir.join("var/nix"), 0o755)?;
    Ok(())
}

fn findmnt(path: &Path) -> Result<bool> {
    command::status_ok("findmnt", &["-rn", path_str(path)?])
}

fn path_str(path: &Path) -> Result<&str> {
    path.to_str()
        .ok_or_else(|| anyhow!("path is not valid UTF-8: {}", path.display()))
}

#[cfg(test)]
#[path = "nix_tests.rs"]
mod tests;
