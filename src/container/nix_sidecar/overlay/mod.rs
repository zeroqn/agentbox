use anyhow::{anyhow, Context, Result};
use std::path::Path;
use std::process::{Command, Stdio};

use crate::container::nix_sidecar::probe::path_is_mounted;
use crate::podman::process::podman_command;

use crate::container::nix_sidecar::PodmanImageMountMode;

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
            let mut podman_unshare = podman_command();
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

pub fn cleanup_merged_mount(merged_dir: &Path, mode: PodmanImageMountMode) -> Result<()> {
    match mode {
        PodmanImageMountMode::Direct => cleanup_current_namespace_mount(merged_dir),
        PodmanImageMountMode::Unshare => cleanup_unshare_namespace_mount(merged_dir),
    }
}

pub fn cleanup_merged_mount_all_namespaces(merged_dir: &Path) -> Result<()> {
    let current_result = cleanup_current_namespace_mount(merged_dir);
    let unshare_result = cleanup_unshare_namespace_mount(merged_dir);

    match (current_result, unshare_result) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(err), Ok(())) | (Ok(()), Err(err)) => Err(err),
        (Err(current_err), Err(unshare_err)) => Err(anyhow!(
            "failed to unmount stale fuse mount '{}' in current namespace ({current_err:#}) and podman unshare namespace ({unshare_err:#})",
            merged_dir.display()
        )),
    }
}

fn cleanup_current_namespace_mount(merged_dir: &Path) -> Result<()> {
    if !path_is_mounted(merged_dir)? {
        return Ok(());
    }

    for (command, args) in current_namespace_unmount_commands() {
        if run_unmount_command(command, &args, merged_dir) {
            return Ok(());
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

fn cleanup_unshare_namespace_mount(merged_dir: &Path) -> Result<()> {
    let status = podman_command()
        .args(build_unshare_cleanup_args(merged_dir))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();

    let status_code = match &status {
        Ok(exit_status) if exit_status.success() => return Ok(()),
        Ok(exit_status) => exit_status.code(),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => None,
    };

    if path_is_mounted(merged_dir)? {
        return cleanup_current_namespace_mount(merged_dir);
    }

    if status_code != Some(2) {
        return Ok(());
    }

    Err(anyhow!(
        "failed to unmount stale fuse mount '{}' in podman unshare namespace",
        merged_dir.display()
    ))
}

fn current_namespace_unmount_commands() -> Vec<(&'static str, Vec<&'static str>)> {
    vec![
        ("fusermount3", vec!["-u"]),
        ("fusermount", vec!["-u"]),
        ("umount", vec![]),
    ]
}

fn run_unmount_command(command: &str, args: &[&str], merged_dir: &Path) -> bool {
    let mut cmd = Command::new(command);
    for arg in args {
        cmd.arg(arg);
    }

    matches!(
        cmd.arg(merged_dir)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status(),
        Ok(exit_status) if exit_status.success()
    )
}

fn build_unshare_cleanup_args(merged_dir: &Path) -> Vec<String> {
    vec![
        "unshare".to_owned(),
        "bash".to_owned(),
        "-lc".to_owned(),
        UNSHARE_CLEANUP_SCRIPT.to_owned(),
        "agentbox".to_owned(),
        merged_dir.display().to_string(),
    ]
}

const UNSHARE_CLEANUP_SCRIPT: &str = r#"set -u
mount_point="$1"
is_mounted() {
  awk -v target="$mount_point" '$5 == target { found=1 } END { exit found ? 0 : 1 }' /proc/self/mountinfo
}
if ! is_mounted; then
  exit 0
fi
if command -v fusermount3 >/dev/null 2>&1 && fusermount3 -u "$mount_point" >/dev/null 2>&1; then
  exit 0
fi
if command -v fusermount >/dev/null 2>&1 && fusermount -u "$mount_point" >/dev/null 2>&1; then
  exit 0
fi
if umount "$mount_point" >/dev/null 2>&1; then
  exit 0
fi
if is_mounted; then
  exit 2
fi
exit 0
"#;

#[cfg(test)]
mod tests;
