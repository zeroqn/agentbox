use anyhow::{Context, Result, anyhow, bail};
use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::runtime::launch_plan::ImageSelection;
use crate::runtime::task_rootfs::{
    BtrfsRootfsCommands, UnsharedBtrfsRootfsCommands, snapshot_mounted_rootfs,
};
use crate::{DEFAULT_FALLBACK_IMAGE, DEFAULT_IMAGE};

const GUEST_INIT_BASENAME: &str = "loftd-guest-init";
const INTERNAL_BTRFS_ROOTFS_COMMAND: &str = "btrfs-rootfs";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BuildahPullPolicy {
    Never,
    Missing,
    Always,
}

impl BuildahPullPolicy {
    fn as_buildah_value(self) -> &'static str {
        match self {
            Self::Never => "never",
            Self::Missing => "missing",
            Self::Always => "always",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ImageSourceAttempt {
    pub(crate) reference: String,
    pub(crate) pull_policy: BuildahPullPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ImageSourceRootfs {
    pub(crate) selected_reference: String,
    pub(crate) image_digest: Option<String>,
    pub(crate) rootfs_path: PathBuf,
}

pub(crate) trait BuildahCommands {
    fn run(&self, args: &[&str]) -> Result<String>;
    fn status(&self, args: &[&str]) -> Result<bool>;
    fn run_unshare_materializer(&self, args: &[&str]) -> Result<String>;
}

trait ChildBuildahCommands {
    fn run(&self, args: &[&str]) -> Result<String>;
}

#[derive(Debug, Clone, Copy)]
struct HostChildBuildahCommands;

impl ChildBuildahCommands for HostChildBuildahCommands {
    fn run(&self, args: &[&str]) -> Result<String> {
        run_buildah(args)
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct HostBuildahCommands;

impl BuildahCommands for HostBuildahCommands {
    fn run(&self, args: &[&str]) -> Result<String> {
        run_buildah(args)
    }

    fn status(&self, args: &[&str]) -> Result<bool> {
        run_buildah_status(args)
    }

    fn run_unshare_materializer(&self, args: &[&str]) -> Result<String> {
        let executable = std::env::current_exe()
            .context("failed to resolve current executable for buildah unshare materialization")?;
        let executable = executable
            .to_str()
            .ok_or_else(|| anyhow!("loftd executable path is not valid UTF-8"))?;
        let mut command_args = vec![
            "unshare".to_owned(),
            executable.to_owned(),
            "internal".to_owned(),
            INTERNAL_BTRFS_ROOTFS_COMMAND.to_owned(),
        ];
        command_args.extend(args.iter().map(|arg| (*arg).to_owned()));
        let borrowed = command_args.iter().map(String::as_str).collect::<Vec<_>>();
        run_buildah(&borrowed)
    }
}

pub(crate) fn materialize_btrfs_source_rootfs(
    selection: &ImageSelection,
    destination: &Path,
    commands: &impl BuildahCommands,
) -> Result<ImageSourceRootfs> {
    commands
        .run(&["--version"])
        .context("failed to verify buildah; btrfs-snapshot task rootfs requires buildah")?;

    let attempt = select_attempt(selection, commands)?;
    let destination = destination
        .to_str()
        .ok_or_else(|| anyhow!("loftd task rootfs path is not valid UTF-8"))?;
    let output = commands
        .run_unshare_materializer(&[
            attempt.reference.as_str(),
            attempt.pull_policy.as_buildah_value(),
            destination,
        ])
        .with_context(|| {
            format!(
                "failed to create btrfs task rootfs from Buildah image source '{}' with pull policy '{}'",
                attempt.reference,
                attempt.pull_policy.as_buildah_value()
            )
        })?;
    parse_materializer_output(&output)
}

fn select_attempt(
    selection: &ImageSelection,
    commands: &impl BuildahCommands,
) -> Result<ImageSourceAttempt> {
    match selection {
        ImageSelection::PreferLocalhostThenCanonical => {
            if commands
                .status(&["inspect", "--type", "image", DEFAULT_IMAGE])
                .with_context(|| format!("failed to inspect local loftd image '{DEFAULT_IMAGE}'"))?
            {
                Ok(ImageSourceAttempt {
                    reference: DEFAULT_IMAGE.to_owned(),
                    pull_policy: BuildahPullPolicy::Never,
                })
            } else {
                Ok(ImageSourceAttempt {
                    reference: DEFAULT_FALLBACK_IMAGE.to_owned(),
                    pull_policy: BuildahPullPolicy::Missing,
                })
            }
        }
        ImageSelection::CanonicalWithRefresh => Ok(ImageSourceAttempt {
            reference: DEFAULT_FALLBACK_IMAGE.to_owned(),
            pull_policy: BuildahPullPolicy::Always,
        }),
        ImageSelection::Explicit { reference } => Ok(ImageSourceAttempt {
            reference: reference.clone(),
            pull_policy: BuildahPullPolicy::Missing,
        }),
    }
}

fn parse_materializer_output(output: &str) -> Result<ImageSourceRootfs> {
    let mut selected_reference = None;
    let mut image_digest = None;
    let mut rootfs_path = None;

    for line in output.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key {
            "selected_image" if !value.is_empty() => selected_reference = Some(value.to_owned()),
            "image_digest" if digest_is_known(value) => image_digest = Some(value.to_owned()),
            "rootfs_path" if !value.is_empty() => rootfs_path = Some(PathBuf::from(value)),
            _ => {}
        }
    }

    Ok(ImageSourceRootfs {
        selected_reference: selected_reference
            .ok_or_else(|| anyhow!("Buildah materializer did not report selected image"))?,
        image_digest,
        rootfs_path: rootfs_path
            .ok_or_else(|| anyhow!("Buildah materializer did not report task rootfs path"))?,
    })
}

fn digest_is_known(value: &str) -> bool {
    !value.is_empty() && value != "<no value>"
}

pub(crate) fn run_internal(args: Vec<OsString>) -> Result<()> {
    let mut args = args.into_iter();
    let command = args
        .next()
        .and_then(|arg| arg.into_string().ok())
        .ok_or_else(|| anyhow!("missing internal command"))?;

    match command.as_str() {
        INTERNAL_BTRFS_ROOTFS_COMMAND => {
            let image_ref = next_utf8_arg(&mut args, "image reference")?;
            let pull_policy = next_utf8_arg(&mut args, "pull policy")?;
            let destination = next_path_arg(&mut args, "destination rootfs")?;
            ensure_no_extra_args(args)?;
            run_btrfs_rootfs_child(&image_ref, &pull_policy, &destination)
        }
        _ => bail!("unknown internal command '{command}'"),
    }
}

fn next_utf8_arg(args: &mut impl Iterator<Item = OsString>, label: &str) -> Result<String> {
    args.next()
        .ok_or_else(|| anyhow!("missing internal {label} argument"))?
        .into_string()
        .map_err(|_| anyhow!("internal {label} argument is not valid UTF-8"))
}

fn next_path_arg(args: &mut impl Iterator<Item = OsString>, label: &str) -> Result<PathBuf> {
    Ok(PathBuf::from(next_utf8_arg(args, label)?))
}

fn ensure_no_extra_args(mut args: impl Iterator<Item = OsString>) -> Result<()> {
    if args.next().is_some() {
        bail!("unexpected extra internal arguments");
    }
    Ok(())
}

fn run_btrfs_rootfs_child(image_ref: &str, pull_policy: &str, destination: &Path) -> Result<()> {
    run_btrfs_rootfs_child_with_commands(
        image_ref,
        pull_policy,
        destination,
        &HostChildBuildahCommands,
        &UnsharedBtrfsRootfsCommands,
    )
}

fn run_btrfs_rootfs_child_with_commands(
    image_ref: &str,
    pull_policy: &str,
    destination: &Path,
    buildah: &impl ChildBuildahCommands,
    btrfs: &impl BtrfsRootfsCommands,
) -> Result<()> {
    let container_id = trim_required(
        buildah.run(&["from", &format!("--pull={pull_policy}"), image_ref])?,
        "buildah from did not return a container id",
    )?;
    let mut container = BuildahContainerGuard::new(buildah, container_id);
    let image_digest = optional_digest(buildah.run(&[
        "inspect",
        "--format",
        "{{.FromImageDigest}}",
        container.id(),
    ])?);
    let mounted_rootfs = PathBuf::from(trim_required(
        buildah.run(&["mount", container.id()])?,
        "buildah mount did not return a mounted rootfs path",
    )?);
    container.mark_mounted();

    find_loftd_guest_init(&mounted_rootfs).with_context(|| {
        format!(
            "Buildah-mounted image rootfs '{}' is not loftd-compatible",
            mounted_rootfs.display()
        )
    })?;
    snapshot_mounted_rootfs(&mounted_rootfs, destination, btrfs).with_context(|| {
        format!(
            "failed to btrfs-snapshot Buildah-mounted rootfs '{}' to '{}'",
            mounted_rootfs.display(),
            destination.display()
        )
    })?;

    println!("selected_image={image_ref}");
    if let Some(digest) = image_digest {
        println!("image_digest={digest}");
    }
    println!("rootfs_path={}", destination.display());
    Ok(())
}

fn run_buildah(args: &[&str]) -> Result<String> {
    let output = Command::new("buildah")
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|err| match err.kind() {
            std::io::ErrorKind::NotFound => anyhow!(
                "buildah is not installed or not available on PATH; loftd btrfs-snapshot task rootfs requires buildah"
            ),
            _ => err.into(),
        })
        .with_context(|| format!("failed to run buildah {}", args.join(" ")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        if stderr.is_empty() {
            bail!("buildah {} failed", args.join(" "));
        }
        bail!("buildah {} failed: {stderr}", args.join(" "));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn run_buildah_status(args: &[&str]) -> Result<bool> {
    let status = match Command::new("buildah")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
    {
        Ok(status) => status,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(err) => {
            return Err(anyhow!(err))
                .with_context(|| format!("failed to run buildah {}", args.join(" ")));
        }
    };
    Ok(status.success())
}

fn trim_required(output: String, empty_message: &'static str) -> Result<String> {
    let trimmed = output.trim();
    if trimmed.is_empty() {
        bail!(empty_message);
    }
    Ok(trimmed.to_owned())
}

fn optional_digest(output: String) -> Option<String> {
    let digest = output.trim();
    digest_is_known(digest).then(|| digest.to_owned())
}

struct BuildahContainerGuard<'a, C: ChildBuildahCommands> {
    commands: &'a C,
    id: String,
    mounted: bool,
}

impl<'a, C: ChildBuildahCommands> BuildahContainerGuard<'a, C> {
    fn new(commands: &'a C, id: String) -> Self {
        Self {
            commands,
            id,
            mounted: false,
        }
    }

    fn id(&self) -> &str {
        &self.id
    }

    fn mark_mounted(&mut self) {
        self.mounted = true;
    }
}

impl<C: ChildBuildahCommands> Drop for BuildahContainerGuard<'_, C> {
    fn drop(&mut self) {
        if self.mounted {
            let _ = self.commands.run(&["umount", &self.id]);
        }
        let _ = self.commands.run(&["rm", &self.id]);
    }
}

pub(crate) fn find_loftd_guest_init(rootfs: &Path) -> Result<PathBuf> {
    let store = rootfs.join("nix/store");
    let mut matches = Vec::new();
    if store.is_dir() {
        for entry in fs::read_dir(&store).with_context(|| {
            format!(
                "failed to read loftd image rootfs store '{}'",
                store.display()
            )
        })? {
            let entry = entry?;
            let candidate = entry.path().join("bin").join(GUEST_INIT_BASENAME);
            if is_executable_file(&candidate) {
                matches.push(candidate);
            }
        }
    }

    match matches.len() {
        1 => Ok(matches.remove(0)),
        0 => bail!(
            "loftd image is not compatible: no executable {GUEST_INIT_BASENAME} found under {}/nix/store/*/bin",
            rootfs.display()
        ),
        count => bail!(
            "loftd image is ambiguous: found {count} executable {GUEST_INIT_BASENAME} binaries under {}/nix/store/*/bin",
            rootfs.display()
        ),
    }
}

fn is_executable_file(path: &Path) -> bool {
    path.metadata()
        .map(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests;
