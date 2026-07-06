use clap::{ArgAction, Parser, Subcommand};
use std::path::PathBuf;

use crate::logging::{LogLevel, LogSettings};
use crate::runtime::landlock::LandlockMode;
use crate::runtime::launch::config::NetworkMode;
use crate::runtime::seccomp::{AuditMode, SeccompCommand, SeccompMode};
use crate::task_rootfs::TaskRootfsBackend;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct VolumeSpec {
    pub(crate) source: PathBuf,
    pub(crate) target: String,
    pub(crate) read_only: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ContainerStoreBackend {
    RawDisk,
}

impl ContainerStoreBackend {
    pub(crate) const DEFAULT: Self = Self::RawDisk;

    pub(crate) fn parse_config_value(value: &str) -> Result<Self, String> {
        match value {
            "raw-disk" => Ok(Self::RawDisk),
            _ => Err("allowed value is raw-disk".to_owned()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PtyMode {
    Normalized,
    RawPassthrough,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PtyOptions {
    pub(crate) mode: PtyMode,
    pub(crate) trace: bool,
    pub(crate) suppress_focus_input: bool,
}

impl PtyOptions {
    pub(crate) const DEFAULT: Self = Self {
        mode: PtyMode::Normalized,
        trace: false,
        suppress_focus_input: false,
    };
}

#[derive(Debug, Clone, Parser, PartialEq, Eq)]
#[command(
    name = "loftd",
    version,
    about = "Launch a direct-libkrun microvm shell with the current directory mounted at /workspace",
    after_help = "Examples:\n  loftd\n  loftd --mem 8\n  loftd --rootfs-backend btrfs-snapshot\n  loftd --rootfs-backend fuse-overlay\n  loftd --container-store raw-disk\n  loftd --guest-init ./loftd-guest-init\n  loftd --profile\n  loftd --root\n  loftd --image ghcr.io/example/loftd:dev\n  LOFTD_IMAGE=ghcr.io/example/loftd:dev loftd\n  loftd -- bash -lc 'echo ok'\n  loftd decode-launch-conf .loftd/.../launch.conf
  loftd --daemon
  loftd --landlock=all -- bash -lc 'echo ok'
  loftd --landlock=best-effort -- bash -lc 'echo ok'
  loftd --seccomp=off -- bash -lc 'echo ok'
  loftd images list
  loftd images sync ghcr.io/example/loftd:dev
  loftd images sync ba5a514
  loftd images remove --dry-run feedfacecafe
  loftd images remove feedfacecafe
  loftd images remove ghcr.io/example/loftd:d
  loftd container-store resize --size 128G
  loftd container-store reset --force
  loftd ps
  loftd attach <task-id-or-handle-selector>
  loftd a <task-id-or-handle-selector>
  loftd kill <task-id-or-handle-selector>
  loftd seccomp synthesize --input trace.jsonl --output policy.json"
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
        long,
        help = "Start the managed task through this TTY, then detach after initial output becomes idle",
        long_help = "Start the managed task through the launching terminal, wait for the guest PTY target to emit initial output and become briefly idle, then detach while leaving the task running for loftd attach. This requires a TTY because terminal initialization queries must be answered by the real terminal."
    )]
    daemon: bool,

    #[arg(
        long = "pty",
        value_name = "MODE[,trace][,no-focus-input]",
        value_parser = parse_pty_arg,
        help = "Configure managed PTY diagnostics for this launched task",
        long_help = "Configure managed PTY diagnostics for this launched task. Allowed mode values are normalize and raw; add trace to enable loftd-terminal.trace and add no-focus-input to suppress host terminal focus reports (ESC[I and ESC[O) before initial-launch stdin is forwarded to the guest. The default is normalize with tracing and focus-input suppression disabled. The trace token writes loftd-terminal.trace in the host current working directory for the new launch; guest-init writes the same workspace file through /workspace/loftd-terminal.trace. Raw, trace, and no-focus-input are independent diagnostics."
    )]
    pty: Option<PtyOptions>,

    #[arg(
        long = "seccomp",
        value_name = "MODE[:POLICY]:PATH",
        value_parser = parse_seccomp_arg,
        help = "Configure host-side loftd seccomp mode for this run",
        long_help = "Configure host-side loftd seccomp mode for this run. Allowed values are off, audit:TRACE_JSONL, trace:TRACE_JSONL, audit:POLICY_JSON:MISSING_TRACE_JSONL, trace:POLICY_JSON:MISSING_TRACE_JSONL, audit-default:MISSING_TRACE_JSONL, trace-default:MISSING_TRACE_JSONL, and enforce:POLICY_JSON. If omitted for a normal task launch, loftd enforces the packaged default policy at $out/share/loftd/seccomp/default.json and fails closed if that policy cannot be loaded; pass --seccomp=off to opt out. Audit mode uses strace/ptrace on the VM worker only to write a tracer-owned record file; gap audit records syscalls missing from the baseline policy by syscall name; audit-default and trace-default use the packaged default policy as the baseline and fail closed if it cannot be loaded; enforce mode applies a seccompiler JSON policy in the VM worker immediately before krun_start_enter."
    )]
    seccomp: Option<SeccompMode>,
    #[arg(
        long = "landlock",
        value_enum,
        value_name = "MODE",
        help = "Configure host-side loftd Landlock mode for this run",
        long_help = "Configure host-side loftd Landlock mode for this run. Allowed values are all, relax, best-effort, and off. If omitted for a normal task launch, loftd uses relax: filesystem, device ioctl, IPC scope, and audit-flag Landlock coverage remain fail-closed, but TCP BindTcp is not handled so guest-local listeners can bind without publishing a host port. all preserves the stricter BindTcp policy and allows only simple TCP --publish-derived host ports. best-effort uses the relax policy shape but applies the supported subset and reports degraded coverage when the current kernel lacks target Landlock features. off disables this host VM-worker Landlock layer. Landlock is applied to the VM worker before seccomp and before krun_start_enter; it does not confine the guest kernel, guest Podman, or helper/network-manager/passt processes started before the VM worker."
    )]
    landlock: Option<LandlockMode>,

    #[arg(
        long,
        help = "Use GrapheneOS hardened_malloc for Nix-linked dynamic binaries",
        long_help = "Use GrapheneOS hardened_malloc for Nix-linked dynamic binaries by asking guest-init to write hardened_malloc to /etc/ld-nix.so.preload. By default, loftd uses mimalloc for Nix-linked dynamic binaries. Foreign/FHS binaries are unchanged."
    )]
    hardened: bool,

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
        long_help = "Override the nested Podman container-store backend for this run. The only allowed value is raw-disk, which is also the default."
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
        short = 'v',
        long = "volume",
        value_name = "SOURCE:TARGET[:ro|:rw]",
        value_parser = parse_volume_arg,
        action = ArgAction::Append,
        help = "Bind-mount a host file or directory into the guest; repeatable",
        long_help = "Bind-mount a host file or directory into the guest; repeatable. Syntax is SOURCE:TARGET, SOURCE:TARGET:rw, or SOURCE:TARGET:ro. Omitted mode defaults to read-write. SELinux, ownership, and propagation options are not supported."
    )]
    volumes: Vec<VolumeSpec>,

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
        name = "container-store",
        about = "Maintain loftd's workspace-scoped raw container-store disk"
    )]
    ContainerStore {
        #[command(subcommand)]
        command: ContainerStoreCommand,
    },

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

    #[command(
        name = "attach",
        visible_alias = "a",
        about = "Attach to an active loftd task VM session"
    )]
    Attach {
        #[arg(value_name = "TASK_ID_OR_HANDLE_SELECTOR")]
        task_id: String,
    },

    #[command(name = "kill", about = "Terminate an active loftd task VM")]
    Kill {
        #[arg(value_name = "TASK_ID_OR_HANDLE_SELECTOR")]
        task_id: String,
    },

    #[command(
        name = "seccomp",
        about = "Audit and synthesize loftd host-side seccomp policies"
    )]
    Seccomp {
        #[command(subcommand)]
        command: SeccompCommand,
    },
}

#[derive(Debug, Clone, Subcommand, PartialEq, Eq)]
pub(crate) enum ContainerStoreCommand {
    #[command(
        name = "resize",
        about = "Grow the raw container-store disk and guest btrfs filesystem"
    )]
    Resize {
        #[arg(
            long = "size",
            value_name = "SIZE",
            help = "New grow-only container-store disk size, for example 128G"
        )]
        size: String,
    },

    #[command(
        name = "reset",
        about = "Delete and recreate the raw container-store disk"
    )]
    Reset {
        #[arg(long = "force", action = ArgAction::SetTrue, help = "Confirm destructive container-store disk reset")]
        force: bool,
    },
}

#[derive(Debug, Clone, Subcommand, PartialEq, Eq)]
pub(crate) enum ImagesCommand {
    #[command(
        name = "sync",
        about = "Sync one Buildah image reference or unique local image selector into loftd's local image cache"
    )]
    Sync {
        #[arg(value_name = "REFERENCE_OR_SELECTOR")]
        reference: String,
    },

    #[command(
        name = "list",
        about = "List loftd's local image cache and Buildah image rows"
    )]
    List,

    #[command(
        name = "remove",
        about = "Remove a loftd image cache entry by unique visible image selector"
    )]
    Remove {
        #[arg(
            long = "dry-run",
            action = ArgAction::SetTrue,
            help = "Preview the exact cache entry and local Buildah image removal without mutating state",
            long_help = "Preview the exact loftd cache entry and final guarded local Buildah image target that would be removed, without mutating cache or local Buildah state. Dry-run is stricter than real remove: it fails when local Buildah removal would be skipped."
        )]
        dry_run: bool,

        #[arg(value_name = "IMAGE_SELECTOR")]
        target: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CliAction {
    Run(RuntimeOptions),
    DecodeLaunchConf {
        path: PathBuf,
    },
    ContainerStore {
        command: ContainerStoreCommand,
        options: ContainerStoreOptions,
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
    Attach {
        task_id: String,
        log_settings: LogSettings,
    },
    Seccomp {
        command: SeccompCommand,
        log_settings: LogSettings,
    },
}

impl Cli {
    pub(crate) fn into_action(self) -> CliAction {
        if let Some(command) = self.command.clone() {
            match command {
                CliCommand::ContainerStore { command } => CliAction::ContainerStore {
                    command,
                    options: self.container_store_options(),
                },
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
                CliCommand::Attach { task_id } => CliAction::Attach {
                    task_id,
                    log_settings: LogSettings::from_process_env(self.log_level, self.debug),
                },
                CliCommand::Seccomp { command } => CliAction::Seccomp {
                    command,
                    log_settings: LogSettings::from_process_env(self.log_level, self.debug),
                },
            }
        } else {
            CliAction::Run(self.into_runtime_options())
        }
    }

    fn container_store_options(&self) -> ContainerStoreOptions {
        ContainerStoreOptions {
            image: self.image.clone(),
            pull_latest: self.pull_latest,
            guest_init: self.guest_init.clone(),
            mem_gib: self.mem_gib,
            log_settings: LogSettings::from_process_env(self.log_level, self.debug),
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
            daemon: self.daemon,
            pty: self.pty.unwrap_or(PtyOptions::DEFAULT),
            seccomp: self.seccomp,
            landlock: self.landlock,
            hardened: self.hardened,
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
            volumes: self.volumes,
            guest_command: self.guest_command,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ContainerStoreOptions {
    pub(crate) image: Option<String>,
    pub(crate) pull_latest: bool,
    pub(crate) guest_init: Option<PathBuf>,
    pub(crate) mem_gib: Option<u32>,
    pub(crate) log_settings: LogSettings,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RuntimeOptions {
    pub(crate) image: Option<String>,
    pub(crate) pull_latest: bool,
    pub(crate) debug: bool,
    pub(crate) log_settings: LogSettings,
    pub(crate) profile: bool,
    pub(crate) root: bool,
    pub(crate) daemon: bool,
    pub(crate) pty: PtyOptions,
    pub(crate) seccomp: Option<SeccompMode>,
    pub(crate) landlock: Option<LandlockMode>,
    pub(crate) hardened: bool,
    pub(crate) rootfs_backend: Option<TaskRootfsBackend>,
    pub(crate) container_store_backend: Option<ContainerStoreBackend>,
    pub(crate) guest_init: Option<PathBuf>,
    pub(crate) preserve_debug: bool,
    pub(crate) mem_gib: Option<u32>,
    pub(crate) network_mode: NetworkMode,
    pub(crate) publish: Vec<String>,
    pub(crate) volumes: Vec<VolumeSpec>,
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

fn parse_pty_arg(value: &str) -> Result<PtyOptions, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err(
            "pty mode must be normalize or raw, optionally followed by ,trace and/or ,no-focus-input"
                .to_owned(),
        );
    }

    let mut mode = None;
    let mut trace = false;
    let mut suppress_focus_input = false;
    for token in value.split(',') {
        let token = token.trim();
        if token.is_empty() {
            return Err("pty mode tokens must not be empty".to_owned());
        }
        match token {
            "normalize" => {
                if mode.replace(PtyMode::Normalized).is_some() {
                    return Err("pty mode must specify exactly one of normalize or raw".to_owned());
                }
            }
            "raw" => {
                if mode.replace(PtyMode::RawPassthrough).is_some() {
                    return Err("pty mode must specify exactly one of normalize or raw".to_owned());
                }
            }
            "trace" => {
                if trace {
                    return Err("pty trace token must not be duplicated".to_owned());
                }
                trace = true;
            }
            "no-focus-input" => {
                if suppress_focus_input {
                    return Err("pty no-focus-input token must not be duplicated".to_owned());
                }
                suppress_focus_input = true;
            }
            other => {
                return Err(format!(
                    "unsupported pty token '{other}'; use normalize, raw, trace, or no-focus-input"
                ));
            }
        }
    }

    Ok(PtyOptions {
        mode: mode.ok_or_else(|| "pty mode must specify normalize or raw".to_owned())?,
        trace,
        suppress_focus_input,
    })
}

fn parse_seccomp_arg(value: &str) -> Result<SeccompMode, String> {
    let value = value.trim();
    if value == "off" {
        return Ok(SeccompMode::Off);
    }
    let parts = value.split(':').collect::<Vec<_>>();
    match parts.as_slice() {
        ["audit" | "trace", trace_path] if !trace_path.trim().is_empty() => {
            Ok(SeccompMode::Audit(AuditMode::Full {
                trace_path: PathBuf::from(trace_path),
            }))
        }
        ["audit-default" | "trace-default", trace_path] if !trace_path.trim().is_empty() => {
            Ok(SeccompMode::Audit(AuditMode::DefaultGap {
                trace_path: PathBuf::from(trace_path),
            }))
        }
        ["audit" | "trace", baseline_policy_path, trace_path]
            if !baseline_policy_path.trim().is_empty() && !trace_path.trim().is_empty() =>
        {
            Ok(SeccompMode::Audit(AuditMode::Gap {
                baseline_policy_path: PathBuf::from(baseline_policy_path),
                trace_path: PathBuf::from(trace_path),
            }))
        }
        ["enforce", policy_path] if !policy_path.trim().is_empty() => Ok(SeccompMode::Enforce {
            policy_path: PathBuf::from(policy_path),
        }),
        ["audit" | "trace", ..] => Err(
            "seccomp audit mode must be audit:TRACE_JSONL or audit:POLICY_JSON:TRACE_JSONL"
                .to_owned(),
        ),
        ["audit-default" | "trace-default", ..] => {
            Err("seccomp default gap audit mode must be audit-default:TRACE_JSONL or trace-default:TRACE_JSONL".to_owned())
        }
        ["enforce", ..] => Err("seccomp enforce mode must be enforce:POLICY_JSON".to_owned()),
        _ => Err(
            "seccomp mode must be off, audit, trace, audit-default, trace-default, or enforce"
                .to_owned(),
        ),
    }
}

fn parse_publish_arg(value: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err("publish spec must not be empty".to_owned());
    }
    Ok(value.to_owned())
}

fn parse_volume_arg(value: &str) -> Result<VolumeSpec, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err("volume spec must not be empty".to_owned());
    }
    let parts = value.split(':').collect::<Vec<_>>();
    if parts.len() < 2 {
        return Err("volume spec must use SOURCE:TARGET syntax".to_owned());
    }
    if parts.len() > 3 {
        return Err(
            "volume spec supports only SOURCE:TARGET, SOURCE:TARGET:ro, or SOURCE:TARGET:rw"
                .to_owned(),
        );
    }
    let source = parts[0].trim();
    let target = parts[1].trim();
    if source.is_empty() {
        return Err("volume source must not be empty".to_owned());
    }
    if target.is_empty() {
        return Err("volume target must not be empty".to_owned());
    }
    let read_only = match parts.get(2).map(|part| part.trim()) {
        None | Some("rw") => false,
        Some("ro") => true,
        Some(option) => {
            return Err(format!(
                "volume option '{option}' is not supported; use ro or rw"
            ));
        }
    };
    Ok(VolumeSpec {
        source: PathBuf::from(source),
        target: target.to_owned(),
        read_only,
    })
}

#[cfg(test)]
mod tests {
    use clap::Parser;
    use std::path::PathBuf;

    use crate::cli::{Cli, ContainerStoreBackend, PtyMode, PtyOptions, VolumeSpec};
    use crate::logging::LogLevel;
    use crate::runtime::landlock::LandlockMode;
    use crate::runtime::launch::config::NetworkMode;
    use crate::runtime::seccomp::{AuditMode, SeccompCommand, SeccompMode};
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
        assert!(!options.daemon);
        assert_eq!(options.seccomp, None);
        assert_eq!(options.landlock, None);
        assert!(options.profile);
        assert!(options.debug);
        assert_eq!(options.pty, PtyOptions::DEFAULT);
        assert!(!options.pty.suppress_focus_input);
        assert_eq!(options.network_mode, NetworkMode::Tsi);
        assert!(options.publish.is_empty());
        assert!(options.volumes.is_empty());
        assert_eq!(options.log_settings.level, LogLevel::Debug);
        assert!(options.guest_command.is_empty());
    }

    #[test]
    fn parses_attach_subcommand() {
        let cli =
            Cli::try_parse_from(["loftd", "attach", "workspace-123"]).expect("attach should parse");
        let alias =
            Cli::try_parse_from(["loftd", "a", "workspace-123"]).expect("a alias should parse");

        assert!(matches!(
            cli.into_action(),
            crate::cli::CliAction::Attach { task_id, .. } if task_id == "workspace-123"
        ));
        assert!(matches!(
            alias.into_action(),
            crate::cli::CliAction::Attach { task_id, .. } if task_id == "workspace-123"
        ));
    }

    #[test]
    fn parses_daemon_runtime_option() {
        let cli =
            Cli::try_parse_from(["loftd", "--daemon"]).expect("daemon runtime option should parse");
        let options = cli.into_runtime_options();

        assert!(options.daemon);
    }

    #[test]
    fn parses_pty_runtime_modes() {
        for (arg, expected) in [
            (
                "normalize",
                PtyOptions {
                    mode: PtyMode::Normalized,
                    trace: false,
                    suppress_focus_input: false,
                },
            ),
            (
                "raw",
                PtyOptions {
                    mode: PtyMode::RawPassthrough,
                    trace: false,
                    suppress_focus_input: false,
                },
            ),
            (
                "normalize,trace",
                PtyOptions {
                    mode: PtyMode::Normalized,
                    trace: true,
                    suppress_focus_input: false,
                },
            ),
            (
                "raw,trace",
                PtyOptions {
                    mode: PtyMode::RawPassthrough,
                    trace: true,
                    suppress_focus_input: false,
                },
            ),
            (
                "normalize,no-focus-input",
                PtyOptions {
                    mode: PtyMode::Normalized,
                    trace: false,
                    suppress_focus_input: true,
                },
            ),
            (
                "raw,no-focus-input,trace",
                PtyOptions {
                    mode: PtyMode::RawPassthrough,
                    trace: true,
                    suppress_focus_input: true,
                },
            ),
        ] {
            let options = Cli::try_parse_from(["loftd", "--pty", arg])
                .expect("pty mode should parse")
                .into_runtime_options();

            assert_eq!(options.pty, expected);
        }
    }

    #[test]
    fn rejects_malformed_pty_runtime_modes() {
        for arg in [
            "",
            "trace",
            "raw,normalize",
            "normalize,raw",
            "raw,raw",
            "raw,trace,trace",
            "normalize,no-focus-input,no-focus-input",
            "no-focus-input",
            "raw,",
            ",raw",
            "passthrough",
        ] {
            let err =
                Cli::try_parse_from(["loftd", "--pty", arg]).expect_err("bad pty mode should fail");
            assert!(matches!(
                err.kind(),
                clap::error::ErrorKind::ValueValidation | clap::error::ErrorKind::InvalidValue
            ));
        }
    }

    #[test]
    fn rejects_removed_pty_raw_passthrough_flag() {
        let err = Cli::try_parse_from(["loftd", "--pty-raw-passthrough"])
            .expect_err("removed raw passthrough flag should fail");

        assert_eq!(err.kind(), clap::error::ErrorKind::UnknownArgument);
    }

    #[test]
    fn pty_option_is_inert_for_attach_subcommand() {
        let cli = Cli::try_parse_from(["loftd", "--pty", "raw,trace", "attach", "workspace-123"])
            .expect("pty option stays parse-compatible for attach");

        assert!(matches!(
            cli.into_action(),
            crate::cli::CliAction::Attach { task_id, .. } if task_id == "workspace-123"
        ));
    }

    #[test]
    fn daemon_is_inert_for_management_subcommands() {
        let cli = Cli::try_parse_from(["loftd", "--daemon", "ps"])
            .expect("daemon stays parse-compatible for management commands");

        assert!(matches!(
            cli.into_action(),
            crate::cli::CliAction::Ps { .. }
        ));
    }

    #[test]
    fn parses_landlock_runtime_modes() {
        for (value, expected) in [
            ("all", LandlockMode::All),
            ("relax", LandlockMode::Relax),
            ("best-effort", LandlockMode::BestEffort),
            ("off", LandlockMode::Off),
        ] {
            let actual = Cli::try_parse_from(["loftd", "--landlock", value])
                .expect("landlock mode should parse")
                .into_runtime_options()
                .landlock;
            assert_eq!(actual, Some(expected));
        }
    }

    #[test]
    fn rejects_malformed_landlock_runtime_modes() {
        for value in ["", "audit", "default", "best_effort", "enforce"] {
            let err = Cli::try_parse_from(["loftd", "--landlock", value])
                .expect_err("bad landlock mode should fail");
            assert!(matches!(
                err.kind(),
                clap::error::ErrorKind::ValueValidation | clap::error::ErrorKind::InvalidValue
            ));
        }
    }

    #[test]
    fn parses_seccomp_runtime_modes() {
        let audit = Cli::try_parse_from(["loftd", "--seccomp", "audit:/tmp/trace.jsonl"])
            .expect("audit seccomp should parse")
            .into_runtime_options()
            .seccomp;
        assert_eq!(
            audit,
            Some(SeccompMode::Audit(AuditMode::Full {
                trace_path: "/tmp/trace.jsonl".into(),
            }))
        );

        let trace_alias = Cli::try_parse_from(["loftd", "--seccomp", "trace:/tmp/trace.jsonl"])
            .expect("trace alias should parse")
            .into_runtime_options()
            .seccomp;
        assert_eq!(
            trace_alias,
            Some(SeccompMode::Audit(AuditMode::Full {
                trace_path: "/tmp/trace.jsonl".into(),
            }))
        );

        let gap_audit = Cli::try_parse_from([
            "loftd",
            "--seccomp",
            "audit:/tmp/baseline.json:/tmp/denied.jsonl",
        ])
        .expect("gap audit seccomp should parse")
        .into_runtime_options()
        .seccomp;
        assert_eq!(
            gap_audit,
            Some(SeccompMode::Audit(AuditMode::Gap {
                baseline_policy_path: "/tmp/baseline.json".into(),
                trace_path: "/tmp/denied.jsonl".into(),
            }))
        );

        let trace_gap_alias = Cli::try_parse_from([
            "loftd",
            "--seccomp",
            "trace:/tmp/baseline.json:/tmp/denied.jsonl",
        ])
        .expect("trace gap alias should parse")
        .into_runtime_options()
        .seccomp;
        assert_eq!(
            trace_gap_alias,
            Some(SeccompMode::Audit(AuditMode::Gap {
                baseline_policy_path: "/tmp/baseline.json".into(),
                trace_path: "/tmp/denied.jsonl".into(),
            }))
        );

        let default_gap_audit =
            Cli::try_parse_from(["loftd", "--seccomp", "audit-default:/tmp/denied.jsonl"])
                .expect("default gap audit seccomp should parse")
                .into_runtime_options()
                .seccomp;
        assert_eq!(
            default_gap_audit,
            Some(SeccompMode::Audit(AuditMode::DefaultGap {
                trace_path: "/tmp/denied.jsonl".into(),
            }))
        );

        let default_trace_alias =
            Cli::try_parse_from(["loftd", "--seccomp", "trace-default:/tmp/denied.jsonl"])
                .expect("default trace alias should parse")
                .into_runtime_options()
                .seccomp;
        assert_eq!(
            default_trace_alias,
            Some(SeccompMode::Audit(AuditMode::DefaultGap {
                trace_path: "/tmp/denied.jsonl".into(),
            }))
        );

        let enforce = Cli::try_parse_from(["loftd", "--seccomp", "enforce:/tmp/policy.json"])
            .expect("enforce seccomp should parse")
            .into_runtime_options()
            .seccomp;
        assert_eq!(
            enforce,
            Some(SeccompMode::Enforce {
                policy_path: "/tmp/policy.json".into(),
            })
        );
    }

    #[test]
    fn seccomp_rejects_malformed_runtime_modes() {
        for args in [
            ["loftd", "--seccomp", ""],
            ["loftd", "--seccomp", "audit:"],
            ["loftd", "--seccomp", "audit::trace.jsonl"],
            ["loftd", "--seccomp", "audit:policy.json:"],
            ["loftd", "--seccomp", "audit:policy.json:trace.jsonl:extra"],
            ["loftd", "--seccomp", "audit-default:"],
            [
                "loftd",
                "--seccomp",
                "audit-default:policy.json:trace.jsonl",
            ],
            ["loftd", "--seccomp", "trace-default:"],
            ["loftd", "--seccomp", "enforce:"],
            ["loftd", "--seccomp", "enforce:/tmp/policy.json:extra"],
            ["loftd", "--seccomp", "default"],
            ["loftd", "--seccomp", "log:/tmp/trace.jsonl"],
        ] {
            let err = Cli::try_parse_from(args).expect_err("bad seccomp mode should fail");
            assert_eq!(err.kind(), clap::error::ErrorKind::ValueValidation);
        }
    }

    #[test]
    fn parses_seccomp_synthesize_subcommand() {
        let cli = Cli::try_parse_from([
            "loftd",
            "seccomp",
            "synthesize",
            "--input",
            "trace.jsonl",
            "--output",
            "policy.json",
        ])
        .expect("seccomp synthesize command should parse");

        assert!(matches!(
            cli.into_action(),
            crate::cli::CliAction::Seccomp { .. }
        ));
    }

    #[test]
    fn parses_seccomp_extend_subcommand() {
        let cli = Cli::try_parse_from([
            "loftd",
            "seccomp",
            "extend",
            "--policy",
            "baseline.json",
            "--trace",
            "denied.jsonl",
            "--output",
            "updated.json",
        ])
        .expect("seccomp extend command should parse");

        match cli.into_action() {
            crate::cli::CliAction::Seccomp {
                command:
                    SeccompCommand::Extend {
                        policy,
                        default_policy,
                        trace,
                        output,
                    },
                ..
            } => {
                assert_eq!(policy, Some(PathBuf::from("baseline.json")));
                assert!(!default_policy);
                assert_eq!(trace, PathBuf::from("denied.jsonl"));
                assert_eq!(output, PathBuf::from("updated.json"));
            }
            other => panic!("expected seccomp extend action, got {other:?}"),
        }
    }

    #[test]
    fn parses_seccomp_extend_default_policy_subcommand() {
        let cli = Cli::try_parse_from([
            "loftd",
            "seccomp",
            "extend",
            "--default-policy",
            "--trace",
            "denied.jsonl",
            "--output",
            "updated.json",
        ])
        .expect("seccomp extend default policy command should parse");

        match cli.into_action() {
            crate::cli::CliAction::Seccomp {
                command:
                    SeccompCommand::Extend {
                        policy,
                        default_policy,
                        trace,
                        output,
                    },
                ..
            } => {
                assert_eq!(policy, None);
                assert!(default_policy);
                assert_eq!(trace, PathBuf::from("denied.jsonl"));
                assert_eq!(output, PathBuf::from("updated.json"));
            }
            other => panic!("expected seccomp extend action, got {other:?}"),
        }
    }

    #[test]
    fn seccomp_extend_requires_exactly_one_baseline_source() {
        let missing = Cli::try_parse_from([
            "loftd",
            "seccomp",
            "extend",
            "--trace",
            "denied.jsonl",
            "--output",
            "updated.json",
        ])
        .expect_err("extend without a baseline source should fail");
        assert_eq!(
            missing.kind(),
            clap::error::ErrorKind::MissingRequiredArgument
        );

        let conflict = Cli::try_parse_from([
            "loftd",
            "seccomp",
            "extend",
            "--policy",
            "baseline.json",
            "--default-policy",
            "--trace",
            "denied.jsonl",
            "--output",
            "updated.json",
        ])
        .expect_err("extend with both baseline sources should fail");
        assert_eq!(conflict.kind(), clap::error::ErrorKind::ArgumentConflict);
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
    fn volume_flag_is_repeatable_and_parses_access_modes() {
        let cli = Cli::try_parse_from([
            "loftd",
            "-v",
            "/host/dir:/guest/dir",
            "--volume",
            "/host/file:/guest/file:ro",
            "-v",
            "/host/cache:/home/dev/cache:rw",
        ])
        .expect("volume flags should parse");
        let options = cli.into_runtime_options();

        assert_eq!(
            options.volumes,
            [
                VolumeSpec {
                    source: "/host/dir".into(),
                    target: "/guest/dir".to_owned(),
                    read_only: false,
                },
                VolumeSpec {
                    source: "/host/file".into(),
                    target: "/guest/file".to_owned(),
                    read_only: true,
                },
                VolumeSpec {
                    source: "/host/cache".into(),
                    target: "/home/dev/cache".to_owned(),
                    read_only: false,
                },
            ]
        );
    }

    #[test]
    fn volume_rejects_empty_or_unsupported_specs() {
        for args in [
            ["loftd", "-v", ""],
            ["loftd", "-v", "/host-only"],
            ["loftd", "-v", ":/guest"],
            ["loftd", "-v", "/host:"],
            ["loftd", "-v", "/host:/guest:z"],
            ["loftd", "-v", "/host:/guest:"],
            ["loftd", "-v", "/host:/guest:ro:rshared"],
        ] {
            let err = Cli::try_parse_from(args).expect_err("volume spec should fail");
            assert_eq!(err.kind(), clap::error::ErrorKind::ValueValidation);
        }
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
    fn parses_hardened_run_option() {
        let cli = Cli::try_parse_from(["loftd", "--hardened"]).expect("hardened should parse");
        let options = cli.into_runtime_options();

        assert!(options.hardened);
    }

    #[test]
    fn hardened_is_inert_for_management_subcommands() {
        let cli = Cli::try_parse_from(["loftd", "--hardened", "images", "list"])
            .expect("hardened stays parse-compatible for management commands");

        match cli.into_action() {
            crate::cli::CliAction::Images { command, .. } => {
                assert_eq!(command, crate::cli::ImagesCommand::List)
            }
            other => panic!("expected images action, got {other:?}"),
        }
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
        assert!(options.volumes.is_empty());
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
    fn container_store_backend_accepts_raw_disk_only() {
        let raw = Cli::try_parse_from(["loftd", "--container-store", "raw-disk"])
            .expect("raw disk container store should parse")
            .into_runtime_options();

        assert_eq!(
            raw.container_store_backend,
            Some(ContainerStoreBackend::RawDisk)
        );
    }

    #[test]
    fn container_store_backend_rejects_invalid_values() {
        let bind_err = Cli::try_parse_from(["loftd", "--container-store", "bind"])
            .expect_err("bind container store should fail");
        let err = Cli::try_parse_from(["loftd", "--container-store", "auto"])
            .expect_err("unknown container store should fail");

        assert_eq!(bind_err.kind(), clap::error::ErrorKind::ValueValidation);
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
        let dry_run =
            Cli::try_parse_from(["loftd", "images", "remove", "--dry-run", "sha256-feedface"])
                .expect("images remove dry-run should parse");

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
                    dry_run: false,
                    target: "sha256-feedface".to_owned()
                }
            ),
            other => panic!("expected images remove action, got {other:?}"),
        }
        match dry_run.into_action() {
            crate::cli::CliAction::Images { command, .. } => assert_eq!(
                command,
                crate::cli::ImagesCommand::Remove {
                    dry_run: true,
                    target: "sha256-feedface".to_owned()
                }
            ),
            other => panic!("expected images remove dry-run action, got {other:?}"),
        }
    }

    #[test]
    fn images_remove_help_mentions_dry_run() {
        let err = Cli::try_parse_from(["loftd", "images", "remove", "--help"])
            .expect_err("help should exit");
        let rendered = err.to_string();

        assert_eq!(err.kind(), clap::error::ErrorKind::DisplayHelp);
        assert!(rendered.contains("--dry-run"));
        assert!(rendered.contains("fails when local Buildah removal would be skipped"));
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
    fn parses_container_store_resize_and_reset_subcommands() {
        let resize = Cli::try_parse_from(["loftd", "container-store", "resize", "--size", "128G"])
            .expect("container-store resize should parse");
        let reset = Cli::try_parse_from(["loftd", "container-store", "reset", "--force"])
            .expect("container-store reset should parse");

        match resize.into_action() {
            crate::cli::CliAction::ContainerStore { command, .. } => assert_eq!(
                command,
                crate::cli::ContainerStoreCommand::Resize {
                    size: "128G".to_owned()
                }
            ),
            other => panic!("expected container-store resize action, got {other:?}"),
        }
        match reset.into_action() {
            crate::cli::CliAction::ContainerStore { command, .. } => assert_eq!(
                command,
                crate::cli::ContainerStoreCommand::Reset { force: true }
            ),
            other => panic!("expected container-store reset action, got {other:?}"),
        }
    }

    #[test]
    fn container_store_reset_accepts_missing_force_for_runtime_error() {
        let reset = Cli::try_parse_from(["loftd", "container-store", "reset"])
            .expect("runtime should produce force-specific error");

        match reset.into_action() {
            crate::cli::CliAction::ContainerStore { command, .. } => assert_eq!(
                command,
                crate::cli::ContainerStoreCommand::Reset { force: false }
            ),
            other => panic!("expected container-store reset action, got {other:?}"),
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
