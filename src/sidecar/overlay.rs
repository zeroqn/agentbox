use anyhow::{anyhow, Context, Result};
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};

use super::PodmanImageMountMode;

pub fn mount_fuse_overlayfs(
    lowerdir: &Path,
    upperdir: &Path,
    workdir: &Path,
    merged: &Path,
    mode: PodmanImageMountMode,
) -> Result<()> {
    let overlay_opts = format!(
        "lowerdir={},upperdir={},workdir={}",
        lowerdir.display(),
        upperdir.display(),
        workdir.display()
    );

    let mut command = Command::new("fuse-overlayfs");
    if mode == PodmanImageMountMode::Unshare {
        command = {
            let mut podman_unshare = Command::new("podman");
            podman_unshare.arg("unshare").arg("fuse-overlayfs");
            podman_unshare
        };
    }

    let status = command
        .arg("-o")
        .arg(&overlay_opts)
        .arg(merged)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|err| match err.kind() {
            std::io::ErrorKind::NotFound => {
                anyhow!("fuse-overlayfs is not installed or not available on PATH")
            }
            _ => err.into(),
        })
        .with_context(|| {
            format!(
                "failed to mount fuse-overlayfs with lowerdir='{}' upperdir='{}' workdir='{}'",
                lowerdir.display(),
                upperdir.display(),
                workdir.display()
            )
        })?;

    if !status.success() {
        return Err(anyhow!(
            "fuse-overlayfs mount failed for '{}' (lower='{}', upper='{}', work='{}')",
            merged.display(),
            lowerdir.display(),
            upperdir.display(),
            workdir.display()
        ));
    }

    Ok(())
}

pub fn cleanup_merged_mount(merged_dir: &Path) -> Result<()> {
    if !path_is_mounted(merged_dir)? {
        return Ok(());
    }

    for (command, args) in [
        ("fusermount3", vec!["-u"]),
        ("fusermount", vec!["-u"]),
        ("umount", vec![]),
        ("podman", vec!["unshare", "fusermount3", "-u"]),
        ("podman", vec!["unshare", "fusermount", "-u"]),
        ("podman", vec!["unshare", "umount"]),
    ] {
        let mut cmd = Command::new(command);
        for arg in &args {
            cmd.arg(arg);
        }
        let status = cmd
            .arg(merged_dir)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();

        match status {
            Ok(exit_status) if exit_status.success() => return Ok(()),
            Ok(_) => continue,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => continue,
        }
    }

    if path_is_mounted(merged_dir)? {
        return Err(anyhow!(
            "failed to unmount stale fuse mount '{}'; unmount it manually before retrying",
            merged_dir.display()
        ));
    }

    Ok(())
}

pub fn path_is_mounted(path: &Path) -> Result<bool> {
    if !path.exists() {
        return Ok(false);
    }

    let target = path
        .canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .to_string();

    let mountinfo = fs::read_to_string("/proc/self/mountinfo")
        .context("failed to read /proc/self/mountinfo for mount health check")?;

    for line in mountinfo.lines() {
        let mut fields = line.split_whitespace();
        let _mount_id = fields.next();
        let _parent_id = fields.next();
        let _major_minor = fields.next();
        let _root = fields.next();
        let mount_point = fields.next();

        if mount_point == Some(target.as_str()) {
            return Ok(true);
        }
    }

    Ok(false)
}
