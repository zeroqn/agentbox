use anyhow::{anyhow, Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

use super::{PodmanImageMountMode, SidecarPaths, SidecarState};

pub fn read_sidecar_state(paths: &SidecarPaths) -> Result<Option<SidecarState>> {
    if !paths.state_file.exists() {
        return Ok(None);
    }

    let contents = fs::read_to_string(&paths.state_file)
        .with_context(|| format!("failed to read '{}'", paths.state_file.display()))?;

    match parse_sidecar_state(&contents, &paths.state_file) {
        Ok(state) => Ok(Some(state)),
        Err(err) => {
            match fs::remove_file(&paths.state_file) {
                Ok(()) => {}
                Err(remove_err) if remove_err.kind() == std::io::ErrorKind::NotFound => {}
                Err(remove_err) => {
                    return Err(remove_err).with_context(|| {
                        format!(
                            "failed to remove stale sidecar state '{}' after parse error: {err:#}",
                            paths.state_file.display()
                        )
                    });
                }
            }
            eprintln!(
                "agentbox: ignored stale sidecar state '{}'; recreating sidecar stack ({err:#})",
                paths.state_file.display()
            );
            Ok(None)
        }
    }
}

pub fn write_sidecar_state(paths: &SidecarPaths, state: &SidecarState) -> Result<()> {
    let parent = paths.state_file.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)
        .with_context(|| format!("failed to create '{}'", parent.display()))?;

    let mount_mode = match state.mount_mode {
        PodmanImageMountMode::Direct => "direct",
        PodmanImageMountMode::Unshare => "unshare",
    };
    let contents = format!(
        "image={}\nimage_id={}\nimage_mount_path={}\nsidecar_name={}\nmount_mode={}\n",
        state.image,
        state.image_id,
        state.image_mount_path.display(),
        state.sidecar_name,
        mount_mode
    );

    fs::write(&paths.state_file, contents)
        .with_context(|| format!("failed to write '{}'", paths.state_file.display()))
}

fn parse_sidecar_state(contents: &str, state_file: &Path) -> Result<SidecarState> {
    let mut image = None;
    let mut image_id = None;
    let mut image_mount_path = None;
    let mut sidecar_name = None;
    let mut mount_mode = None;

    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        if let Some((key, value)) = trimmed.split_once('=') {
            match key {
                "image" => image = Some(value.to_owned()),
                "image_id" => image_id = Some(value.to_owned()),
                "image_mount_path" => image_mount_path = Some(PathBuf::from(value)),
                "sidecar_name" => sidecar_name = Some(value.to_owned()),
                "mount_mode" => {
                    mount_mode = Some(match value {
                        "direct" => PodmanImageMountMode::Direct,
                        "unshare" => PodmanImageMountMode::Unshare,
                        _ => {
                            return Err(anyhow!(
                                "unsupported mount_mode '{}' in '{}'",
                                value,
                                state_file.display()
                            ))
                        }
                    })
                }
                _ => {}
            }
        }
    }

    match (image, image_id, image_mount_path, sidecar_name) {
        (Some(image), Some(image_id), Some(image_mount_path), Some(sidecar_name)) => {
            Ok(SidecarState {
                image,
                image_id,
                image_mount_path,
                sidecar_name,
                mount_mode: mount_mode.unwrap_or(PodmanImageMountMode::Direct),
            })
        }
        _ => Err(anyhow!("'{}' is incomplete", state_file.display())),
    }
}
