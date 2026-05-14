use anyhow::{anyhow, bail, Context, Result};
use std::path::Path;
use std::time::Duration;

use crate::guest_init::command;
use crate::guest_init::components::env::LibkrunEnv;
use crate::guest_init::profile::ProfileRecorder;
use crate::guest_init::{fs, process};

pub(in crate::guest_init) const SOCKET_PATH: &str = "/nix/var/nix/daemon-socket/socket";

const PROFILE_REQUIRE_TOOLS: &str = "bootstrap-nix:require-tools";
const PROFILE_FIND_DISK: &str = "bootstrap-nix:find-disk";
const PROFILE_PREPARE_RUN_DIRS: &str = "bootstrap-nix:prepare-run-dirs";
const PROFILE_MOUNT_DISK: &str = "bootstrap-nix:mount-disk";
const PROFILE_PRESEED_UPPER: &str = "bootstrap-nix:preseed-upper";
const PROFILE_MOUNT_OVERLAY: &str = "bootstrap-nix:mount-overlay";
const PROFILE_CREATE_SOCKET_DIR: &str = "bootstrap-nix:create-socket-dir";
const PROFILE_START_DAEMON: &str = "bootstrap-nix:start-daemon";
const PROFILE_WAIT_SOCKET: &str = "bootstrap-nix:wait-socket";

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::guest_init) enum NixOperation {
    FindDisk,
    MountDisk,
    PreseedUpper,
    MountOverlay,
    StartDaemon,
    WaitSocket,
}

#[cfg(test)]
pub(in crate::guest_init) fn planned_operations() -> Vec<NixOperation> {
    vec![
        NixOperation::FindDisk,
        NixOperation::MountDisk,
        NixOperation::PreseedUpper,
        NixOperation::MountOverlay,
        NixOperation::StartDaemon,
        NixOperation::WaitSocket,
    ]
}

#[cfg(test)]
pub(in crate::guest_init) fn planned_profile_labels() -> Vec<&'static str> {
    vec![
        PROFILE_REQUIRE_TOOLS,
        PROFILE_FIND_DISK,
        PROFILE_PREPARE_RUN_DIRS,
        PROFILE_MOUNT_DISK,
        PROFILE_PRESEED_UPPER,
        PROFILE_MOUNT_OVERLAY,
        PROFILE_CREATE_SOCKET_DIR,
        PROFILE_START_DAEMON,
        PROFILE_WAIT_SOCKET,
    ]
}

pub(in crate::guest_init) fn bootstrap(
    env_contract: &LibkrunEnv,
    profiler: &mut impl ProfileRecorder,
) -> Result<()> {
    if !env_contract.nix_overlay {
        return Ok(());
    }
    if !process::is_root() {
        bail!("libkrun /nix overlay bootstrap must run as root");
    }
    profiler.measure_result(PROFILE_REQUIRE_TOOLS, || {
        for tool in ["blkid", "mount", "findmnt", "nix-daemon"] {
            command::require_on_path(tool)?;
        }
        Ok(())
    })?;

    let disk = profiler.measure_result(PROFILE_FIND_DISK, || {
        crate::guest_init::components::disk::nix::find_disk(
            &env_contract.nix_disk_label,
            &env_contract.nix_disk_id,
        )
    })?;
    let lower_dir = Path::new("/nix");
    let run_dir = Path::new("/run/agentbox");
    let disk_mount = run_dir.join("nix-disk");
    let upper_dir = disk_mount.join("upper");
    let work_dir = disk_mount.join("work");

    profiler.measure_result(PROFILE_PREPARE_RUN_DIRS, || {
        fs::create_dir_all(run_dir)?;
        fs::create_dir_all(&disk_mount)?;
        Ok(())
    })?;

    profiler.measure_result(PROFILE_MOUNT_DISK, || {
        if !findmnt(&disk_mount)? {
            command::run(
                "mount",
                &["-t", "btrfs", path_str(&disk)?, path_str(&disk_mount)?],
            )
            .context("failed to mount libkrun /nix btrfs disk")?;
        }
        Ok(())
    })?;

    profiler.measure_result(PROFILE_PRESEED_UPPER, || {
        preseed_upper(&lower_dir, &upper_dir, &work_dir)
    })?;
    let options = format!(
        "lowerdir={},upperdir={},workdir={}",
        lower_dir.display(),
        upper_dir.display(),
        work_dir.display()
    );
    profiler.measure_result(PROFILE_MOUNT_OVERLAY, || {
        command::run(
            "mount",
            &["-t", "overlay", "overlay", "-o", &options, "/nix"],
        )
        .context("failed to mount libkrun overlay at /nix")
    })?;

    profiler.measure_result(PROFILE_CREATE_SOCKET_DIR, || {
        fs::create_dir_all(Path::new("/nix/var/nix/daemon-socket"))
    })?;
    let mut child = profiler.measure_result(PROFILE_START_DAEMON, || {
        command::spawn_background("nix-daemon", &["--daemon"]).context("failed to start nix-daemon")
    })?;
    profiler.measure_result(PROFILE_WAIT_SOCKET, || {
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
    })
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
#[path = "root_tests.rs"]
mod tests;
