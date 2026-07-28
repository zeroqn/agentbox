use anyhow::Result;
use std::path::{Component, Path, PathBuf};

use crate::logging::LogLevel;
use crate::runtime::landlock::LandlockMode;
use crate::runtime::seccomp::SeccompMode;
use crate::runtime::session::rootfs::image_source::OciProcessConfig;
use crate::runtime::vm::gpu::GpuMode;

pub(crate) const DEFAULT_WAYPIPE_PORT: u32 = 50_427;
pub(crate) const WORKSPACE_TAG: &str = "loftd-workspace";
pub(crate) const WORKSPACE_TARGET: &str = "/workspace";
pub const CODEX_TAG: &str = "loftd-codex";
pub const CODEX_TARGET: &str = "/home/dev/.codex";
pub const PI_TAG: &str = "loftd-pi";
pub const PI_TARGET: &str = "/home/dev/.pi";
pub const OMP_TAG: &str = "loftd-omp";
pub const OMP_TARGET: &str = "/home/dev/.omp";
pub const DIRGE_CONFIG_TAG: &str = "loftd-dirge-config";
pub const DIRGE_CONFIG_TARGET: &str = "/home/dev/.config/dirge";
pub const DIRGE_DATA_TAG: &str = "loftd-dirge-data";
pub const DIRGE_DATA_TARGET: &str = "/home/dev/.local/share/dirge";
pub const DIRGE_HOME_TAG: &str = "loftd-dirge-home";
pub const DIRGE_HOME_TARGET: &str = "/home/dev/.dirge";
pub const CARGO_TAG: &str = "loftd-cargo";
pub const CARGO_TARGET: &str = "/home/dev/.cargo";
pub const SCCACHE_TAG: &str = "loftd-sccache";
pub const SCCACHE_TARGET: &str = "/home/dev/.cache/sccache";
pub const NIX_TAG: &str = "loftd-nix";
pub const NIX_TARGET: &str = "/nix";
pub(super) const SCCACHE_DIR_ENV: &str = "SCCACHE_DIR";
pub(super) const HOST_UID_ENV: &str = "LOFTD_HOST_UID";
pub(super) const HOST_GID_ENV: &str = "LOFTD_HOST_GID";
pub(super) const ENTER_AS_ROOT_ENV: &str = "LOFTD_ENTER_AS_ROOT";
pub(super) const GUEST_PROFILE_ENV: &str = "LOFTD_GUEST_PROFILE";
pub(super) const GUEST_DEBUG_ENV: &str = "LOFTD_GUEST_DEBUG";
pub(super) const NIX_ALLOCATOR_ENV: &str = "LOFTD_NIX_ALLOCATOR";
pub(super) const GUEST_USE_PASST_ENV: &str = "LOFTD_USE_PASST";
pub(super) const GUEST_WAYLAND_ENV: &str = "LOFTD_WAYLAND";
pub(super) const GUEST_WAYPIPE_PORT_ENV: &str = "LOFTD_WAYPIPE_PORT";
pub(super) const GUEST_EXEC_PORT_ENV: &str = "LOFTD_EXEC_PORT";
pub(super) const GUEST_EXEC_PROTOCOL_VERSION_ENV: &str = "LOFTD_EXEC_PROTOCOL_VERSION";
pub(super) const GUEST_IO_URING_ENV: &str = "LOFTD_IO_URING";
pub(super) const GUEST_PERF_ENV: &str = "LOFTD_PERF";
pub(super) const GUEST_SESSION_MANAGED_ENV: &str = "LOFTD_SESSION_MANAGED";
pub(super) const GUEST_ATTACH_PORT_ENV: &str = "LOFTD_ATTACH_PORT";
pub(super) const GUEST_ATTACH_PROTOCOL_VERSION_ENV: &str = "LOFTD_ATTACH_PROTOCOL_VERSION";
pub(super) const IMAGE_PATH_ENV: &str = "PATH";
pub(super) const KRUN_CONFIG_ENV: &str = "KRUN_CONFIG";
pub(crate) const LOFTD_KRUN_CONFIG_PATH: &str = "/.loftd_config.json";
pub(super) const KIB: u64 = 1024;
pub(super) const MIB_PER_GIB: u32 = 1024;
pub(super) const BYTES_PER_GIB: u64 = 1024 * 1024 * 1024;
pub(super) const MAX_GIB_FOR_KRUN_RAM_MIB: u32 = u32::MAX / MIB_PER_GIB;
pub(super) const HOST_MEMINFO: &str = "/proc/meminfo";
pub(super) const IMAGE_LOFTD_ENV_ALLOWLIST: &[&str] = &[
    "NIX_CONFIG",
    "SSL_CERT_FILE",
    "NIX_SSL_CERT_FILE",
    "LOFTD_FISH_CONFIG_SOURCE",
    "LOFTD_STARSHIP_CONFIG_SOURCE",
    "LOFTD_MIMALLOC_LIB",
    "LOFTD_GRAPHENE_HARDENED_MALLOC_LIB",
    "LOFTD_REAL_PODMAN",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindMount {
    pub source: PathBuf,
    pub tag: String,
    pub target: String,
    pub source_kind: BindMountSourceKind,
    pub read_only: bool,
}

impl BindMount {
    pub(crate) fn directory(
        source: impl Into<PathBuf>,
        tag: impl Into<String>,
        target: impl Into<String>,
    ) -> Self {
        Self {
            source: source.into(),
            tag: tag.into(),
            target: target.into(),
            source_kind: BindMountSourceKind::Directory,
            read_only: false,
        }
    }

    pub(crate) fn file(
        source: impl Into<PathBuf>,
        tag: impl Into<String>,
        target: impl Into<String>,
        read_only: bool,
    ) -> Self {
        Self {
            source: source.into(),
            tag: tag.into(),
            target: target.into(),
            source_kind: BindMountSourceKind::File,
            read_only,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BindMountSourceKind {
    Directory,
    File,
}

impl BindMountSourceKind {
    pub(crate) fn as_config_value(self) -> &'static str {
        match self {
            Self::Directory => "directory",
            Self::File => "file",
        }
    }

    pub(crate) fn parse_config_value(value: &str) -> Result<Self> {
        match value {
            "directory" => Ok(Self::Directory),
            "file" => Ok(Self::File),
            _ => anyhow::bail!("loftd launch config bind mount source_kind is invalid"),
        }
    }
}

pub(crate) fn canonical_mount_target(target: &str) -> Result<String> {
    let path = Path::new(target);
    if !path.is_absolute() {
        anyhow::bail!("loftd bind mount target '{target}' must be absolute");
    }
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            Component::RootDir => {}
            Component::Normal(value) => {
                let value = value.to_str().ok_or_else(|| {
                    anyhow::anyhow!("loftd bind mount target '{target}' must be valid UTF-8")
                })?;
                components.push(value);
            }
            Component::CurDir => {}
            Component::ParentDir => {
                anyhow::bail!("loftd bind mount target '{target}' must not contain '..'");
            }
            Component::Prefix(_) => {
                anyhow::bail!("loftd bind mount target '{target}' has an unsupported prefix");
            }
        }
    }
    if components.is_empty() {
        anyhow::bail!("loftd bind mount target must not be /");
    }
    Ok(format!("/{}", components.join("/")))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AllocatorMode {
    Mimalloc,
    Hardened,
    Glibc,
}

impl AllocatorMode {
    pub(crate) fn as_env_value(self) -> &'static str {
        match self {
            Self::Mimalloc => "mimalloc",
            Self::Hardened => "hardened",
            Self::Glibc => "glibc",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GuestInitOverrideMount {
    pub(crate) source: PathBuf,
    pub(crate) target: String,
    pub(crate) read_only: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DiskAttachment {
    pub(crate) id: String,
    pub(crate) path: PathBuf,
    pub(crate) read_only: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HostNixOverlay {
    pub(crate) selected_reference: String,
    pub(crate) image_digest: String,
    pub(crate) digest_key: String,
    pub(crate) lowerdir: PathBuf,
    pub(crate) upperdir: PathBuf,
    pub(crate) workdir: PathBuf,
    pub(crate) mergeddir: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NetworkMode {
    Tsi,
    Passt,
}

impl NetworkMode {
    pub(crate) fn as_config_value(self) -> &'static str {
        match self {
            Self::Tsi => "tsi",
            Self::Passt => "passt",
        }
    }

    pub(super) fn parse_config_value(value: &str) -> Result<Self> {
        match value {
            "tsi" => Ok(Self::Tsi),
            "passt" => Ok(Self::Passt),
            _ => anyhow::bail!("loftd launch config network_mode is invalid"),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct LaunchSpec<'a> {
    pub(crate) task_rootfs: &'a Path,
    pub(crate) hostname: &'a str,
    pub mounts: &'a [BindMount],
    pub(crate) guest_init_override: Option<GuestInitOverrideMount>,
    pub(crate) guest_init_exec: &'a str,
    pub(crate) guest_command: &'a [String],
    pub(crate) image_process_config: &'a OciProcessConfig,
    pub(crate) mem_gib: Option<u32>,
    pub(crate) log_level: LogLevel,
    pub(crate) network_mode: NetworkMode,
    pub(crate) gpu_mode: GpuMode,
    pub(crate) wayland: bool,
    pub(crate) io_uring: bool,
    pub(crate) perf: bool,
    pub(crate) publish: &'a [String],
    pub(crate) profile: bool,
    pub(crate) root: bool,
    pub(crate) allocator: AllocatorMode,
    pub(crate) host_uid: u32,
    pub(crate) host_gid: u32,
    pub(crate) vcpus: u8,
    pub(crate) disks: Vec<DiskAttachment>,
    pub(crate) extra_env: Vec<(String, String)>,
    pub(crate) host_nix_overlay: Option<HostNixOverlay>,
    pub(crate) waypipe: Option<WaypipeConfig>,
    pub(crate) exec: Option<ExecConfig>,
    pub(crate) managed_session: Option<ManagedSessionConfig>,
}

/// Serialized helper/libkrun execution contract.
///
/// `LaunchConfig` is derived from a resolved launch plan plus materialized task
/// rootfs data and is written into the task rootfs for the helper process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WaypipeConfig {
    pub(crate) socket: PathBuf,
    pub(crate) guest_port: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExecConfig {
    pub(crate) socket: PathBuf,
    pub(crate) guest_port: u32,
    pub(crate) protocol_version: u16,
    pub(crate) socket_uid: u32,
    pub(crate) socket_gid: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ManagedSessionConfig {
    pub(crate) attach_socket: PathBuf,
    pub(crate) guest_port: u32,
    pub(crate) protocol_version: u16,
    pub(crate) attach_socket_uid: u32,
    pub(crate) attach_socket_gid: u32,
    pub(crate) cleanup_task_rootfs_on_exit: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LaunchConfig {
    pub(crate) task_rootfs: PathBuf,
    pub(crate) hostname: String,
    pub mounts: Vec<BindMount>,
    pub(crate) host_nix_overlay: Option<HostNixOverlay>,
    pub(crate) guest_init_override: Option<GuestInitOverrideMount>,
    pub(crate) disks: Vec<DiskAttachment>,
    pub(crate) ram_mib: u32,
    pub(crate) vcpus: u8,
    pub(crate) log_level: LogLevel,
    pub(crate) network_mode: NetworkMode,
    pub(crate) gpu_mode: GpuMode,
    pub(crate) io_uring: bool,
    pub(crate) perf: bool,
    pub(crate) publish: Vec<String>,
    pub(crate) workdir: String,
    pub(crate) exec_path: String,
    pub(crate) argv: Vec<String>,
    pub(crate) env: Vec<(String, String)>,
    pub(crate) guest_config_env: Vec<(String, String)>,
    pub(crate) passt_fd: Option<i32>,
    pub(crate) waypipe: Option<WaypipeConfig>,
    pub(crate) exec: Option<ExecConfig>,
    pub(crate) managed_session: Option<ManagedSessionConfig>,
    pub(crate) seccomp: SeccompMode,
    pub(crate) landlock: LandlockMode,
}
