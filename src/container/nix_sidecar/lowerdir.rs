use anyhow::{anyhow, Context, Result};
use std::path::{Path, PathBuf};

use crate::container::nix_sidecar::types::PodmanImageMountMode;
use crate::podman::command::run_podman_output;
use crate::NIX_STORE_DIR;

pub fn resolve_sidecar_lowerdir(image_mount_path: &Path) -> Result<PathBuf> {
    let nested_nix = image_mount_path.join("nix");
    if nested_nix.is_dir() {
        return Ok(nested_nix);
    }

    let root_store = image_mount_path.join(NIX_STORE_DIR);
    if root_store.is_dir() {
        return Ok(image_mount_path.to_path_buf());
    }

    Err(anyhow!(
        "expected either '{}' or '{}' to exist as directories",
        nested_nix.display(),
        root_store.display()
    ))
}

pub fn resolve_sidecar_lowerdir_for_mode(
    image_mount_path: &Path,
    mode: PodmanImageMountMode,
) -> Result<PathBuf> {
    if mode == PodmanImageMountMode::Direct {
        return resolve_sidecar_lowerdir(image_mount_path);
    }

    let mount_path = image_mount_path.to_str().with_context(|| {
        format!(
            "image mount path '{}' is not valid UTF-8",
            image_mount_path.display()
        )
    })?;
    let script = "mount_path=\"$1\"\nif [ -d \"$mount_path/nix\" ]; then\n  printf '%s\\n' \"$mount_path/nix\"\nelif [ -d \"$mount_path/store\" ]; then\n  printf '%s\\n' \"$mount_path\"\nelse\n  exit 3\nfi";
    let args = vec![
        "unshare".to_owned(),
        "bash".to_owned(),
        "-lc".to_owned(),
        script.to_owned(),
        "agentbox".to_owned(),
        mount_path.to_owned(),
    ];
    let output = run_podman_output(args, "failed to resolve sidecar lowerdir in podman unshare")?;
    let lowerdir = output.trim();
    if lowerdir.is_empty() {
        return Err(anyhow!(
            "podman unshare lowerdir probe returned empty output for '{}'",
            image_mount_path.display()
        ));
    }

    Ok(PathBuf::from(lowerdir))
}
