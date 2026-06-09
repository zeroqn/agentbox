use clap::{ArgAction, Parser, Subcommand};
use std::path::PathBuf;

use crate::logging::{LogLevel, LogSettings};
use crate::runtime::launch::config::NetworkMode;
use crate::task_rootfs::TaskRootfsBackend;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ContainerStoreBackend {
    Bind,
    RawDisk,
}

impl ContainerStoreBackend {
    pub(crate) const DEFAULT: Self = Self::Bind;

    pub(crate) fn parse_config_value(value: &str) -> Result<Self, String> {
        match value {
            "bind" => Ok(Self::Bind),
            "raw-disk" => Ok(Self::RawDisk),
            _ => Err("allowed values are bind and raw-disk".to_owned()),
        }
    }
}

#[derive(Debug, Clone, Parser, PartialEq, Eq)]
#[command(
    name = "loftd",
    version,
    about = "Launch a direct-libkrun microvm shell with the current directory mounted at /workspace",
    after_help = "Examples:\n  loftd\n  loftd --mem 8\n  loftd --rootfs-backend btrfs-snapshot\n  loftd --rootfs-backend fuse-overlay\n  loftd --container-store raw-disk\n  loftd --guest-init ./loftd-guest-init\n  loftd --profile\n  loftd --root\n  loftd --image ghcr.io/example/loftd:dev\n  LOFTD_IMAGE=ghcr.io/example/loftd:dev loftd\n  loftd -- bash -lc 'echo ok'\n  loftd decode-launch-conf .loftd/.../launch.conf
  loftd images list
  loftd images sync ghcr.io/example/loftd:dev
  loftd images remove sha256-feedface
  loftd ps
  loftd kill <task-id>"
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
        long_help = "Compatibility flag for loftd debug logging. Equivalent to --log-level debug when --log-level/LOFTD_LOG_LEVEL is not set."
    )]
    debug: bool,

    #[arg(
        long = "log-level",
        env = "LOFTD_LOG_LEVEL",
        value_enum,
        value_name = "LEVEL",
        help = "Set loftd/libkrun diagnostic log level",
        long_help = "Set loftd and helper diagnostic log level. Allowed values are off, error, warn, info, debug, and trace. CLI --log-level overrides LOFTD_LOG_LEVEL; --debug remains a compatibility alias for debug when neither is set."
    )]
    log_level: Option<LogLevel>,

    #[arg(
        long,
        help = "Enable loftd component timing collection",
        long_help = "Enable loftd component timing collection. Timing reports are emitted to stderr when profiling is requested, so normal command stdout remains reserved for command output."
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
        long = "container-store",
        value_name = "BACKEND",
        value_parser = parse_container_store_backend_arg,
        help = "Override the nested Podman container-store backend for this run",
        long_help = "Override the nested Podman container-store backend for this run. Allowed values are bind and raw-disk. If omitted, loftd bind-mounts a persistent host state directory into the guest."
    )]
    container_store_backend: Option<ContainerStoreBackend>,

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

    #[arg(
        long = "passt",
        help = "Use libkrun virtio-net/passt mode instead of the default TSI mode"
    )]
    passt: bool,

    #[arg(
        short = 'p',
        long = "publish",
        value_name = "SPEC",
        value_parser = parse_publish_arg,
        action = ArgAction::Append,
        help = "Publish a host port to the guest; repeatable",
        long_help = "Publish a host port to the guest; repeatable. Default TSI mode accepts simple TCP HOST_PORT:GUEST_PORT. With --passt, tcp:SPEC and udp:SPEC select passt TCP/UDP forwarding, and unprefixed SPEC defaults to TCP."
    )]
    publish: Vec<String>,

    #[arg(
        value_name = "COMMAND",
        last = true,
        num_args = 1..,
        allow_hyphen_values = true,
        help = "Run command inside the guest instead of the default fish login shell"
    )]
    guest_command: Vec<String>,

    #[command(subcommand)]
    command: Option<CliCommand>,
}

#[derive(Debug, Clone, Subcommand, PartialEq, Eq)]
pub(crate) enum CliCommand {
    #[command(
        name = "decode-launch-conf",
        about = "Decode a hex-encoded loftd launch.conf for debugging"
    )]
    DecodeLaunchConf {
        #[arg(value_name = "PATH")]
        path: PathBuf,
    },

    #[command(
        name = "images",
        about = "Manage loftd's local Buildah-backed image snapshot cache"
    )]
    Images {
        #[command(subcommand)]
        command: ImagesCommand,
    },

    #[command(name = "ps", about = "List active loftd task VMs across workspaces")]
    Ps,

    #[command(name = "kill", about = "Terminate an active loftd task VM")]
    Kill {
        #[arg(value_name = "TASK_ID")]
        task_id: String,
    },
}

#[derive(Debug, Clone, Subcommand, PartialEq, Eq)]
pub(crate) enum ImagesCommand {
    #[command(
        name = "sync",
        about = "Sync one Buildah image reference into loftd's local image cache"
    )]
    Sync {
        #[arg(value_name = "REFERENCE")]
        reference: String,
    },

    #[command(name = "list", about = "List loftd's local image cache entries")]
    List,

    #[command(
        name = "remove",
        about = "Remove a loftd image cache entry by digest or digest key"
    )]
    Remove {
        #[arg(value_name = "DIGEST_OR_KEY")]
        target: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CliAction {
    Run(RuntimeOptions),
    DecodeLaunchConf {
        path: PathBuf,
    },
    Images {
        command: ImagesCommand,
        log_settings: LogSettings,
    },
    Ps {
        log_settings: LogSettings,
    },
    Kill {
        task_id: String,
        log_settings: LogSettings,
    },
}

impl Cli {
    pub(crate) fn into_action(self) -> CliAction {
        if let Some(command) = self.command.clone() {
            match command {
                CliCommand::DecodeLaunchConf { path } => CliAction::DecodeLaunchConf { path },
                CliCommand::Images { command } => CliAction::Images {
                    command,
                    log_settings: LogSettings::from_process_env(self.log_level, self.debug),
                },
                CliCommand::Ps => CliAction::Ps {
                    log_settings: LogSettings::from_process_env(self.log_level, self.debug),
                },
                CliCommand::Kill { task_id } => CliAction::Kill {
                    task_id,
                    log_settings: LogSettings::from_process_env(self.log_level, self.debug),
                },
            }
        } else {
            CliAction::Run(self.into_runtime_options())
        }
    }

    pub(crate) fn into_runtime_options(self) -> RuntimeOptions {
        let log_settings = LogSettings::from_process_env(self.log_level, self.debug);
        RuntimeOptions {
            image: self.image,
            pull_latest: self.pull_latest,
            debug: self.debug,
            log_settings,
            profile: self.profile,
            root: self.root,
            rootfs_backend: self.rootfs_backend,
            container_store_backend: self.container_store_backend,
            guest_init: self.guest_init,
            preserve_debug: self.preserve_debug,
            mem_gib: self.mem_gib,
            network_mode: if self.passt {
                NetworkMode::Passt
            } else {
                NetworkMode::Tsi
            },
            publish: self.publish,
            guest_command: self.guest_command,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RuntimeOptions {
    pub(crate) image: Option<String>,
    pub(crate) pull_latest: bool,
    pub(crate) debug: bool,
    pub(crate) log_settings: LogSettings,
    pub(crate) profile: bool,
    pub(crate) root: bool,
    pub(crate) rootfs_backend: Option<TaskRootfsBackend>,
    pub(crate) container_store_backend: Option<ContainerStoreBackend>,
    pub(crate) guest_init: Option<PathBuf>,
    pub(crate) preserve_debug: bool,
    pub(crate) mem_gib: Option<u32>,
    pub(crate) network_mode: NetworkMode,
    pub(crate) publish: Vec<String>,
    pub(crate) guest_command: Vec<String>,
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

fn parse_container_store_backend_arg(value: &str) -> Result<ContainerStoreBackend, String> {
    ContainerStoreBackend::parse_config_value(value)
}

fn parse_publish_arg(value: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err("publish spec must not be empty".to_owned());
    }
    Ok(value.to_owned())
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use crate::cli::{Cli, ContainerStoreBackend};
    use crate::logging::LogLevel;
    use crate::runtime::launch::config::NetworkMode;
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
        assert_eq!(options.container_store_backend, None);
        assert_eq!(options.mem_gib, Some(8));
        assert_eq!(
            options.guest_init.as_deref(),
            Some("./loftd-guest-init".as_ref())
        );
        assert!(options.preserve_debug);
        assert!(options.root);
        assert!(options.profile);
        assert!(options.debug);
        assert_eq!(options.network_mode, NetworkMode::Tsi);
        assert!(options.publish.is_empty());
        assert_eq!(options.log_settings.level, LogLevel::Debug);
        assert!(options.guest_command.is_empty());
    }

    #[test]
    fn passt_flag_selects_passt_network_mode() {
        let cli = Cli::try_parse_from(["loftd", "--passt"]).expect("passt flag should parse");
        let options = cli.into_runtime_options();

        assert_eq!(options.network_mode, NetworkMode::Passt);
    }

    #[test]
    fn publish_flag_is_repeatable() {
        let cli = Cli::try_parse_from(["loftd", "-p", "8080:80", "--publish", "8443:443"])
            .expect("publish flags should parse");
        let options = cli.into_runtime_options();

        assert_eq!(options.publish, ["8080:80", "8443:443"]);
    }

    #[test]
    fn publish_rejects_empty_spec() {
        let empty_err =
            Cli::try_parse_from(["loftd", "-p", ""]).expect_err("empty publish should fail");
        let whitespace_err = Cli::try_parse_from(["loftd", "--publish", "   "])
            .expect_err("blank publish should fail");

        assert_eq!(empty_err.kind(), clap::error::ErrorKind::ValueValidation);
        assert_eq!(
            whitespace_err.kind(),
            clap::error::ErrorKind::ValueValidation
        );
    }

    #[test]
    fn parses_explicit_guest_command_after_delimiter() {
        let cli = Cli::try_parse_from(["loftd", "--", "bash", "-lc", "echo ok"])
            .expect("guest command should parse after delimiter");
        let options = cli.into_runtime_options();

        assert_eq!(options.guest_command, ["bash", "-lc", "echo ok"]);
    }

    #[test]
    fn publish_preserves_guest_command() {
        let cli = Cli::try_parse_from(["loftd", "-p", "8080:80", "--", "bash", "-lc", "echo ok"])
            .expect("publish and command should parse");
        let options = cli.into_runtime_options();

        assert_eq!(options.publish, ["8080:80"]);
        assert_eq!(options.guest_command, ["bash", "-lc", "echo ok"]);
    }

    #[test]
    fn parses_explicit_guest_command_after_options_and_delimiter() {
        let cli = Cli::try_parse_from([
            "loftd",
            "--guest-init",
            "/tmp/loftd-guest-init",
            "--log-level",
            "debug",
            "--profile",
            "--",
            "sh",
            "/workspace/probe.sh",
        ])
        .expect("guest command should parse after options and delimiter");
        let options = cli.into_runtime_options();

        assert_eq!(
            options.guest_init.as_deref(),
            Some("/tmp/loftd-guest-init".as_ref())
        );
        assert_eq!(options.log_settings.level, LogLevel::Debug);
        assert!(options.profile);
        assert_eq!(options.guest_command, ["sh", "/workspace/probe.sh"]);
    }

    #[test]
    fn parses_decode_launch_conf_subcommand() {
        let cli = Cli::try_parse_from(["loftd", "decode-launch-conf", "/tmp/launch.conf"])
            .expect("decode subcommand should parse");

        assert_eq!(
            cli.into_action(),
            crate::cli::CliAction::DecodeLaunchConf {
                path: "/tmp/launch.conf".into(),
            }
        );
    }

    #[test]
    fn decode_launch_conf_is_not_confused_with_guest_command() {
        let cli = Cli::try_parse_from(["loftd", "--", "decode-launch-conf", "/tmp/launch.conf"])
            .expect("delimited words should remain a guest command");
        let options = cli.into_runtime_options();

        assert_eq!(
            options.guest_command,
            ["decode-launch-conf", "/tmp/launch.conf"]
        );
    }

    #[test]
    fn bare_words_are_not_guest_commands() {
        let err = Cli::try_parse_from(["loftd", "microvm"])
            .expect_err("guest commands must use an explicit delimiter");

        assert_eq!(err.kind(), clap::error::ErrorKind::UnknownArgument);
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
    fn container_store_backend_accepts_bind_and_raw_disk() {
        let bind = Cli::try_parse_from(["loftd", "--container-store", "bind"])
            .expect("bind container store should parse")
            .into_runtime_options();
        let raw = Cli::try_parse_from(["loftd", "--container-store", "raw-disk"])
            .expect("raw disk container store should parse")
            .into_runtime_options();

        assert_eq!(
            bind.container_store_backend,
            Some(ContainerStoreBackend::Bind)
        );
        assert_eq!(
            raw.container_store_backend,
            Some(ContainerStoreBackend::RawDisk)
        );
    }

    #[test]
    fn container_store_backend_rejects_invalid_values() {
        let err = Cli::try_parse_from(["loftd", "--container-store", "auto"])
            .expect_err("unknown container store should fail");

        assert_eq!(err.kind(), clap::error::ErrorKind::ValueValidation);
    }

    #[test]
    fn legacy_storage_flag_is_not_accepted() {
        let err = Cli::try_parse_from(["loftd", "--storage", "auto"])
            .expect_err("storage flag should not exist");

        assert_eq!(err.kind(), clap::error::ErrorKind::UnknownArgument);
    }

    #[test]
    fn parses_explicit_log_level() {
        let cli =
            Cli::try_parse_from(["loftd", "--log-level", "trace"]).expect("log level should parse");
        let options = cli.into_runtime_options();

        assert_eq!(options.log_settings.level, LogLevel::Trace);
    }

    #[test]
    fn rejects_invalid_log_level() {
        let err = Cli::try_parse_from(["loftd", "--log-level", "verbose"])
            .expect_err("unknown log level should fail");

        assert_eq!(err.kind(), clap::error::ErrorKind::InvalidValue);
    }

    #[test]
    fn explicit_log_level_overrides_debug_compatibility() {
        let cli = Cli::try_parse_from(["loftd", "--debug", "--log-level", "info"])
            .expect("log level should parse");
        let options = cli.into_runtime_options();

        assert!(options.debug);
        assert_eq!(options.log_settings.level, LogLevel::Info);
    }

    #[test]
    fn memory_must_be_positive_gib() {
        let err =
            Cli::try_parse_from(["loftd", "--mem", "0"]).expect_err("zero memory should fail");

        assert_eq!(err.kind(), clap::error::ErrorKind::ValueValidation);
    }
    #[test]
    fn parses_images_sync_subcommand() {
        let cli = Cli::try_parse_from(["loftd", "images", "sync", "ghcr.io/example/loftd:dev"])
            .expect("images sync should parse");

        match cli.into_action() {
            crate::cli::CliAction::Images { command, .. } => assert_eq!(
                command,
                crate::cli::ImagesCommand::Sync {
                    reference: "ghcr.io/example/loftd:dev".to_owned()
                }
            ),
            other => panic!("expected images action, got {other:?}"),
        }
    }

    #[test]
    fn parses_images_list_and_remove_subcommands() {
        let list =
            Cli::try_parse_from(["loftd", "images", "list"]).expect("images list should parse");
        let remove = Cli::try_parse_from(["loftd", "images", "remove", "sha256-feedface"])
            .expect("images remove should parse");

        match list.into_action() {
            crate::cli::CliAction::Images { command, .. } => {
                assert_eq!(command, crate::cli::ImagesCommand::List);
            }
            other => panic!("expected images list action, got {other:?}"),
        }
        match remove.into_action() {
            crate::cli::CliAction::Images { command, .. } => assert_eq!(
                command,
                crate::cli::ImagesCommand::Remove {
                    target: "sha256-feedface".to_owned()
                }
            ),
            other => panic!("expected images remove action, got {other:?}"),
        }
    }

    #[test]
    fn parses_task_control_ps_and_kill_subcommands() {
        let ps = Cli::try_parse_from(["loftd", "ps"]).expect("ps should parse");
        let kill =
            Cli::try_parse_from(["loftd", "kill", "workspace-1-42"]).expect("kill should parse");

        match ps.into_action() {
            crate::cli::CliAction::Ps { .. } => {}
            other => panic!("expected ps action, got {other:?}"),
        }
        match kill.into_action() {
            crate::cli::CliAction::Kill { task_id, .. } => {
                assert_eq!(task_id, "workspace-1-42");
            }
            other => panic!("expected kill action, got {other:?}"),
        }
    }

    #[test]
    fn images_words_after_delimiter_remain_guest_command() {
        let cli = Cli::try_parse_from(["loftd", "--", "images", "list"])
            .expect("delimited images words should parse as guest command");
        let options = cli.into_runtime_options();

        assert_eq!(options.guest_command, ["images", "list"]);
    }
}
