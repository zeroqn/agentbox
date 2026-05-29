use clap::{Parser, ValueEnum};
use std::path::PathBuf;

use crate::{DEFAULT_FALLBACK_IMAGE, DEFAULT_IMAGE};

#[derive(Debug, Clone, Parser, PartialEq, Eq)]
#[command(
    name = "loftd",
    version,
    about = "Launch a direct-libkrun microvm shell with the current directory mounted at /workspace",
    after_help = "Examples:\n  loftd\n  loftd --mem 8\n  loftd --storage auto\n  loftd --guest-init ./loftd-init\n  loftd --profile --debug\n  loftd --root\n  loftd --image ghcr.io/example/loftd:dev\n  LOFTD_IMAGE=ghcr.io/example/loftd:dev loftd"
)]
pub(crate) struct Cli {
    #[arg(
        long,
        env = "LOFTD_IMAGE",
        help = "Container image to run",
        long_help = "Container image to run. If omitted, loftd prefers localhost/loftd:latest and falls back to ghcr.io/zeroqn/loftd:latest. Can also be set with LOFTD_IMAGE."
    )]
    image: Option<String>,

    #[arg(
        long,
        help = "Refresh and use ghcr.io/zeroqn/loftd:latest for this run",
        long_help = "Refresh and use ghcr.io/zeroqn/loftd:latest for this run when --image is not set. The future runtime implementation will perform this refresh through Buildah, not host Podman."
    )]
    pull_latest: bool,

    #[arg(
        long,
        help = "Enable loftd debug logging",
        long_help = "Enable loftd debug logging for host-side lifecycle diagnostics. This does not enable host Podman because loftd does not use host Podman directly."
    )]
    debug: bool,

    #[arg(
        long,
        help = "Enable loftd component timing collection",
        long_help = "Enable loftd component timing collection. Timing is reported to stderr only when --debug is also set, so normal command stdout remains reserved for command output."
    )]
    profile: bool,

    #[arg(
        long,
        help = "Enter the task shell as root",
        long_help = "Enter the task shell as root instead of dropping to the host/dev identity. By default, loftd drops privileges for the interactive shell."
    )]
    root: bool,

    #[arg(
        long = "storage",
        value_enum,
        default_value = "auto",
        help = "Select the microvm task rootfs storage backend"
    )]
    storage: StoragePolicy,

    #[arg(
        long = "guest-init",
        value_name = "PATH",
        help = "Override loftd-init in the microvm image"
    )]
    guest_init: Option<PathBuf>,

    #[arg(long = "preserve-debug", help = "Preserve task debug state after exit")]
    preserve_debug: bool,

    #[arg(
        long = "mem",
        value_name = "GiB",
        value_parser = parse_mem_gib_arg,
        help = "Set microvm memory in GiB"
    )]
    mem_gib: Option<u32>,
}

impl Cli {
    pub(crate) fn into_runtime_options(self) -> RuntimeOptions {
        RuntimeOptions {
            image: self.image,
            image_resolution: resolve_image_strategy(self.pull_latest),
            debug: self.debug,
            profile: self.profile,
            root: self.root,
            storage: self.storage,
            guest_init: self.guest_init,
            preserve_debug: self.preserve_debug,
            mem_gib: self.mem_gib,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RuntimeOptions {
    pub(crate) image: Option<String>,
    pub(crate) image_resolution: ImageResolutionStrategy,
    pub(crate) debug: bool,
    pub(crate) profile: bool,
    pub(crate) root: bool,
    pub(crate) storage: StoragePolicy,
    pub(crate) guest_init: Option<PathBuf>,
    pub(crate) preserve_debug: bool,
    pub(crate) mem_gib: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum StoragePolicy {
    Auto,
    #[value(name = "btrfs-snapshot")]
    BtrfsSnapshot,
    Reflink,
    #[value(name = "fuse-overlay")]
    FuseOverlay,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ImageResolutionStrategy {
    PullLatestGhcr,
    PreferLocalhostFallback,
}

pub(crate) fn parse_mem_gib_arg(value: &str) -> Result<u32, String> {
    let mem_gib = value
        .parse::<u32>()
        .map_err(|_| "memory must be a positive integer GiB value".to_owned())?;

    if mem_gib == 0 {
        return Err("memory must be greater than 0 GiB".to_owned());
    }

    Ok(mem_gib)
}

fn resolve_image_strategy(pull_latest: bool) -> ImageResolutionStrategy {
    if pull_latest {
        ImageResolutionStrategy::PullLatestGhcr
    } else {
        ImageResolutionStrategy::PreferLocalhostFallback
    }
}

pub(crate) fn selected_image_reference(
    explicit_image: Option<&str>,
    strategy: ImageResolutionStrategy,
) -> &str {
    match explicit_image {
        Some(image) => image,
        None => match strategy {
            ImageResolutionStrategy::PullLatestGhcr => DEFAULT_FALLBACK_IMAGE,
            ImageResolutionStrategy::PreferLocalhostFallback => DEFAULT_IMAGE,
        },
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use crate::cli::{Cli, ImageResolutionStrategy, StoragePolicy, selected_image_reference};
    use crate::{DEFAULT_FALLBACK_IMAGE, DEFAULT_IMAGE};

    #[test]
    fn parses_single_runtime_options_without_subcommand() {
        let cli = Cli::try_parse_from([
            "loftd",
            "--storage",
            "fuse-overlay",
            "--mem",
            "8",
            "--guest-init",
            "./loftd-init",
            "--preserve-debug",
            "--root",
            "--profile",
            "--debug",
        ])
        .expect("single runtime options should parse");
        let options = cli.into_runtime_options();

        assert_eq!(options.storage, StoragePolicy::FuseOverlay);
        assert_eq!(options.mem_gib, Some(8));
        assert_eq!(options.guest_init.as_deref(), Some("./loftd-init".as_ref()));
        assert!(options.preserve_debug);
        assert!(options.root);
        assert!(options.profile);
        assert!(options.debug);
    }

    #[test]
    fn image_env_uses_loftd_prefix() {
        let cli = Cli::try_parse_from(["loftd", "--image", "example/loftd:dev"])
            .expect("image should parse");
        let options = cli.into_runtime_options();

        assert_eq!(options.image.as_deref(), Some("example/loftd:dev"));
    }

    #[test]
    fn pull_latest_selects_ghcr_default_without_host_podman() {
        let cli = Cli::try_parse_from(["loftd", "--pull-latest"]).expect("pull flag should parse");
        let options = cli.into_runtime_options();

        assert_eq!(
            options.image_resolution,
            ImageResolutionStrategy::PullLatestGhcr
        );
        assert_eq!(
            selected_image_reference(options.image.as_deref(), options.image_resolution),
            DEFAULT_FALLBACK_IMAGE
        );
    }

    #[test]
    fn default_image_prefers_local_loftd_reference() {
        assert_eq!(
            selected_image_reference(None, ImageResolutionStrategy::PreferLocalhostFallback),
            DEFAULT_IMAGE
        );
    }

    #[test]
    fn memory_must_be_positive_gib() {
        let err =
            Cli::try_parse_from(["loftd", "--mem", "0"]).expect_err("zero memory should fail");

        assert_eq!(err.kind(), clap::error::ErrorKind::ValueValidation);
    }
}
