use anyhow::{anyhow, Result};
use std::path::PathBuf;
use std::process::Stdio;

use crate::podman::command::{run_podman, run_podman_output};
use crate::podman::unshare::build_podman_unshare_args;

use super::{resolve_sidecar_lowerdir_for_mode, PodmanImageMountMode};

pub fn inspect_image_id(image: &str) -> Result<String> {
    let args = vec![
        "image".to_owned(),
        "inspect".to_owned(),
        "--format".to_owned(),
        "{{.Id}}".to_owned(),
        image.to_owned(),
    ];
    let output = run_podman_output(args, "failed to inspect image metadata")?;
    let image_id = output.trim();
    if image_id.is_empty() {
        return Err(anyhow!(
            "podman image inspect returned an empty image ID for '{}'",
            image
        ));
    }
    Ok(image_id.to_owned())
}

pub fn mount_image_with_lowerdir(image: &str) -> Result<(PathBuf, PathBuf, PodmanImageMountMode)> {
    let mut attempts = Vec::new();

    for mode in [PodmanImageMountMode::Direct, PodmanImageMountMode::Unshare] {
        match mount_image_once(image, mode) {
            Ok(image_mount_path) => {
                match resolve_sidecar_lowerdir_for_mode(&image_mount_path, mode) {
                    Ok(lowerdir) => return Ok((image_mount_path, lowerdir, mode)),
                    Err(err) => {
                        let _ = unmount_image_mode(image, mode);
                        attempts.push(format!(
                            "{} returned '{}' without a usable lowerdir: {err}",
                            mode.label(),
                            image_mount_path.display(),
                        ));
                    }
                }
            }
            Err(err) => {
                attempts.push(format!("{} failed: {err:#}", mode.label()));
            }
        }
    }

    Err(anyhow!(
        "unable to mount image '{}' with a usable Nix lowerdir; attempts: {}",
        image,
        attempts.join(" | ")
    ))
}

pub fn unmount_image(image: &str) -> Result<()> {
    for mode in [PodmanImageMountMode::Direct, PodmanImageMountMode::Unshare] {
        let _ = unmount_image_mode(image, mode);
    }
    Ok(())
}

fn build_podman_image_mount_args(image: &str, mode: PodmanImageMountMode) -> Vec<String> {
    let args = vec!["image".to_owned(), "mount".to_owned(), image.to_owned()];
    match mode {
        PodmanImageMountMode::Direct => args,
        PodmanImageMountMode::Unshare => build_podman_unshare_args(args),
    }
}

fn build_podman_image_unmount_args(image: &str, mode: PodmanImageMountMode) -> Vec<String> {
    let args = vec!["image".to_owned(), "unmount".to_owned(), image.to_owned()];
    match mode {
        PodmanImageMountMode::Direct => args,
        PodmanImageMountMode::Unshare => build_podman_unshare_args(args),
    }
}

fn mount_image_once(image: &str, mode: PodmanImageMountMode) -> Result<PathBuf> {
    let args = build_podman_image_mount_args(image, mode);
    let output = run_podman_output(args, "failed to mount image rootfs")?;
    let mount_path = output
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .map(str::trim)
        .ok_or_else(|| anyhow!("podman image mount returned no mount path for '{}'", image))?;

    let path = PathBuf::from(mount_path);
    if !path.is_dir() {
        return Err(anyhow!(
            "podman image mount path '{}' is not a directory",
            path.display()
        ));
    }

    Ok(path)
}

fn unmount_image_mode(image: &str, mode: PodmanImageMountMode) -> Result<()> {
    let args = build_podman_image_unmount_args(image, mode);
    let _ = run_podman(
        args,
        Stdio::null(),
        Stdio::null(),
        Stdio::null(),
        "failed to unmount image",
    );
    Ok(())
}

#[cfg(test)]
mod tests;
