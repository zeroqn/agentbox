use clap::Parser;
use std::path::PathBuf;

use crate::task_rootfs::TaskRootfsBackend;

#[derive(Debug, Clone, Parser, PartialEq, Eq)]
#[command(
    name = "loftd",
    version,
    about = "Launch a direct-libkrun microvm shell with the current directory mounted at /workspace",
    after_help = "Examples:\n  loftd\n  loftd --mem 8\n  loftd --rootfs-backend btrfs-snapshot\n  loftd --rootfs-backend fuse-overlay\n  loftd --guest-init ./loftd-guest-init\n  loftd --profile --debug\n  loftd --root\n  loftd --image ghcr.io/example/loftd:dev\n  LOFTD_IMAGE=ghcr.io/example/loftd:dev loftd"
)]
pub(crate) struct Cli {
    #[arg(
        long,
        env = "LOFTD_IMAGE",
        conflicts_with = "pull_latest",
        help = "Container image to run",
        long_help = "Container image to run. If omitted, loftd prefers localhost/loftd:latest and can fall back to ghcr.io/zeroqn/loftd:latest in a future image-ingestion slice. Can also be set with LOFTD_IMAGE."
    )]
    image: Option<String>,

    #[arg(
        long,
        conflicts_with = "image",
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
        long = "rootfs-backend",
        value_name = "BACKEND",
        value_parser = parse_rootfs_backend_arg,
        help = "Override the task rootfs backend for this run",
        long_help = "Override the task rootfs backend for this run. Allowed values are btrfs-snapshot and fuse-overlay. If omitted, loftd uses [task-rootfs].backend from loftd.toml or defaults to btrfs-snapshot."
    )]
    rootfs_backend: Option<TaskRootfsBackend>,

    #[arg(
        long = "guest-init",
        value_name = "PATH",
        help = "Override loftd-guest-init in the microvm image"
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
            pull_latest: self.pull_latest,
            debug: self.debug,
            profile: self.profile,
            root: self.root,
            rootfs_backend: self.rootfs_backend,
            guest_init: self.guest_init,
            preserve_debug: self.preserve_debug,
            mem_gib: self.mem_gib,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RuntimeOptions {
    pub(crate) image: Option<String>,
    pub(crate) pull_latest: bool,
    pub(crate) debug: bool,
    pub(crate) profile: bool,
    pub(crate) root: bool,
    pub(crate) rootfs_backend: Option<TaskRootfsBackend>,
    pub(crate) guest_init: Option<PathBuf>,
    pub(crate) preserve_debug: bool,
    pub(crate) mem_gib: Option<u32>,
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

fn parse_rootfs_backend_arg(value: &str) -> Result<TaskRootfsBackend, String> {
    TaskRootfsBackend::parse_config_value(value)
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use crate::cli::Cli;
    use crate::task_rootfs::TaskRootfsBackend;

    #[test]
    fn parses_single_runtime_options_without_subcommand() {
        let cli = Cli::try_parse_from([
            "loftd",
            "--rootfs-backend",
            "fuse-overlay",
            "--mem",
            "8",
            "--guest-init",
            "./loftd-guest-init",
            "--preserve-debug",
            "--root",
            "--profile",
            "--debug",
        ])
        .expect("single runtime options should parse");
        let options = cli.into_runtime_options();

        assert_eq!(options.rootfs_backend, Some(TaskRootfsBackend::FuseOverlay));
        assert_eq!(options.mem_gib, Some(8));
        assert_eq!(
            options.guest_init.as_deref(),
            Some("./loftd-guest-init".as_ref())
        );
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
    fn pull_latest_records_canonical_refresh_intent() {
        let cli = Cli::try_parse_from(["loftd", "--pull-latest"]).expect("pull flag should parse");
        let options = cli.into_runtime_options();

        assert!(options.pull_latest);
        assert_eq!(options.image, None);
    }

    #[test]
    fn image_and_pull_latest_are_mutually_exclusive() {
        let err = Cli::try_parse_from(["loftd", "--image", "example/loftd:dev", "--pull-latest"])
            .expect_err("explicit image and canonical refresh should conflict");

        assert_eq!(err.kind(), clap::error::ErrorKind::ArgumentConflict);
    }

    #[test]
    fn rootfs_backend_rejects_removed_values() {
        let auto_err = Cli::try_parse_from(["loftd", "--rootfs-backend", "auto"])
            .expect_err("auto backend should fail");
        let reflink_err = Cli::try_parse_from(["loftd", "--rootfs-backend", "reflink"])
            .expect_err("reflink backend should fail");

        assert_eq!(auto_err.kind(), clap::error::ErrorKind::ValueValidation);
        assert_eq!(reflink_err.kind(), clap::error::ErrorKind::ValueValidation);
    }

    #[test]
    fn legacy_storage_flag_is_not_accepted() {
        let err = Cli::try_parse_from(["loftd", "--storage", "auto"])
            .expect_err("storage flag should not exist");

        assert_eq!(err.kind(), clap::error::ErrorKind::UnknownArgument);
    }

    #[test]
    fn memory_must_be_positive_gib() {
        let err =
            Cli::try_parse_from(["loftd", "--mem", "0"]).expect_err("zero memory should fail");

        assert_eq!(err.kind(), clap::error::ErrorKind::ValueValidation);
    }
}
