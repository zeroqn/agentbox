use anyhow::{anyhow, Context, Result};
use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::{
    build_podman_unshare_args, run_podman, run_podman_output, NIX_STORE_DIR, SIDECAR_NAME_PREFIX,
    SIDECAR_NAME_SLUG_FALLBACK, SIDECAR_NAME_SLUG_MAX_LEN,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PodmanImageMountMode {
    Direct,
    Unshare,
}

impl PodmanImageMountMode {
    pub(crate) fn state_value(self) -> &'static str {
        match self {
            Self::Direct => "direct",
            Self::Unshare => "unshare",
        }
    }

    pub(crate) fn from_state_value(value: &str) -> Option<Self> {
        match value {
            "direct" => Some(Self::Direct),
            "unshare" => Some(Self::Unshare),
            _ => None,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Direct => "podman image mount",
            Self::Unshare => "podman unshare podman image mount",
        }
    }
}

pub(crate) fn derive_sidecar_name(cwd: &Path, image_id: &str) -> String {
    let workspace_slug = derive_sidecar_workspace_slug(cwd);
    let mut hasher = DefaultHasher::new();
    cwd.hash(&mut hasher);
    image_id.hash(&mut hasher);
    let digest = hasher.finish();
    format!("{SIDECAR_NAME_PREFIX}-{workspace_slug}-{digest:016x}")
}

fn derive_sidecar_workspace_slug(cwd: &Path) -> String {
    let workspace_name = cwd
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or(SIDECAR_NAME_SLUG_FALLBACK);

    let mut slug = String::new();
    let mut last_was_separator = false;

    for ch in workspace_name.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            last_was_separator = false;
        } else if !slug.is_empty() && !last_was_separator {
            slug.push('-');
            last_was_separator = true;
        }
    }

    let truncated = slug
        .trim_matches('-')
        .chars()
        .take(SIDECAR_NAME_SLUG_MAX_LEN)
        .collect::<String>();
    let trimmed = truncated.trim_matches('-');

    if trimmed.is_empty() {
        SIDECAR_NAME_SLUG_FALLBACK.to_owned()
    } else {
        trimmed.to_owned()
    }
}

pub(crate) fn inspect_image_id(image: &str) -> Result<String> {
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

pub(crate) fn build_podman_image_mount_args(
    image: &str,
    mode: PodmanImageMountMode,
) -> Vec<String> {
    let args = vec!["image".to_owned(), "mount".to_owned(), image.to_owned()];
    match mode {
        PodmanImageMountMode::Direct => args,
        PodmanImageMountMode::Unshare => build_podman_unshare_args(args),
    }
}

pub(crate) fn build_podman_image_unmount_args(
    image: &str,
    mode: PodmanImageMountMode,
) -> Vec<String> {
    let args = vec!["image".to_owned(), "unmount".to_owned(), image.to_owned()];
    match mode {
        PodmanImageMountMode::Direct => args,
        PodmanImageMountMode::Unshare => build_podman_unshare_args(args),
    }
}

pub(crate) fn mount_image_with_lowerdir(
    image: &str,
) -> Result<(PathBuf, PathBuf, PodmanImageMountMode)> {
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

pub(crate) fn unmount_image(image: &str) -> Result<()> {
    for mode in [PodmanImageMountMode::Direct, PodmanImageMountMode::Unshare] {
        let _ = unmount_image_mode(image, mode);
    }
    Ok(())
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

pub(crate) fn mount_fuse_overlayfs(
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

pub(crate) fn resolve_sidecar_lowerdir(image_mount_path: &Path) -> Result<PathBuf> {
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

pub(crate) fn resolve_sidecar_lowerdir_for_mode(
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

pub(crate) fn cleanup_sidecar_container(sidecar_name: &str) -> Result<()> {
    let args = vec!["rm".to_owned(), "-f".to_owned(), sidecar_name.to_owned()];
    let _ = run_podman(
        args,
        Stdio::null(),
        Stdio::null(),
        Stdio::null(),
        "failed to remove stale sidecar container",
    );
    Ok(())
}

pub(crate) fn cleanup_merged_mount(merged_dir: &Path) -> Result<()> {
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

pub(crate) fn path_is_mounted(path: &Path) -> Result<bool> {
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
