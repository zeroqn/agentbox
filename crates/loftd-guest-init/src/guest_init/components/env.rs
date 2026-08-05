use anyhow::{Context, Result, anyhow};
use std::env;
use std::str::FromStr;

pub(in crate::guest_init) const DEV_USER: &str = "dev";
pub(in crate::guest_init) const DEV_HOME: &str = "/home/dev";
pub(in crate::guest_init) const DEFAULT_SHELL: &str = "fish";
pub(in crate::guest_init) const RUN_DIR: &str = "/run/loftd";
pub(in crate::guest_init) const NIX_STATUS_PATH: &str = "/run/loftd/nix-prep.status";
pub(in crate::guest_init) const NIX_LOG_PATH: &str = "/run/loftd/nix-prep.log";
pub(in crate::guest_init) const NIX_WAIT_TIMEOUT_SECS: u64 = 120;
pub(in crate::guest_init) const NIX_DAEMON_SOCKET_PATH: &str = "/nix/var/nix/daemon-socket/socket";
pub(in crate::guest_init) const NIX_REMOTE_URI: &str = "unix:///nix/var/nix/daemon-socket/socket";
pub(in crate::guest_init) const PODMAN_STATUS_PATH: &str = "/run/loftd/podman-prep.status";
pub(in crate::guest_init) const PODMAN_LOG_PATH: &str = "/run/loftd/podman-prep.log";
pub(in crate::guest_init) const PODMAN_WAIT_TIMEOUT_SECS: u64 = 120;
pub(in crate::guest_init) const RAW_NIX_DISK_ID: &str = "loftd-nix";
pub(in crate::guest_init) const RAW_NIX_DISK_LABEL: &str = "LOFTD_NIX";
pub(in crate::guest_init) const RAW_CONTAINER_DISK_ID: &str = "loftd-containers";
pub(in crate::guest_init) const RAW_CONTAINER_DISK_LABEL: &str = "LOFTD_CONTAINERS";
pub(in crate::guest_init) const ENTER_AS_ROOT_ENV: &str = "LOFTD_ENTER_AS_ROOT";
pub(in crate::guest_init) const CONTAINERS_STORE_ENV: &str = "LOFTD_CONTAINERS_STORE";
const LEGACY_NIX_OVERLAY_ENV: &str = "AGENTBOX_LIBKRUN_NIX_OVERLAY";
const LEGACY_CONTAINERS_STORAGE_ENV: &str = "AGENTBOX_LIBKRUN_CONTAINERS_STORAGE";
const LEGACY_USE_PASST_ENV: &str = "AGENTBOX_LIBKRUN_USE_PASST";
const LEGACY_ENTER_AS_ROOT_ENV: &str = "AGENTBOX_ENTER_AS_ROOT";
const LEGACY_HOST_UID_ENV: &str = "AGENTBOX_HOST_UID";
const LEGACY_HOST_GID_ENV: &str = "AGENTBOX_HOST_GID";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::guest_init) enum ContainerStoreBackend {
    RawDisk,
}

impl ContainerStoreBackend {
    pub(in crate::guest_init) fn parse_env_value(value: &str) -> Result<Self> {
        match value {
            "raw-disk" => Ok(Self::RawDisk),
            _ => anyhow::bail!("{CONTAINERS_STORE_ENV} must be 'raw-disk' (got '{value}')"),
        }
    }

    pub(in crate::guest_init) fn from_optional_env_value(value: Option<String>) -> Result<Self> {
        value.map_or(Ok(Self::RawDisk), |value| Self::parse_env_value(&value))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::guest_init) enum GuestPermission {
    IoUring,
    NetAdmin,
    NetRaw,
    Bpf,
    Perf,
    SysAdmin,
}

impl GuestPermission {
    const ALL: [Self; 6] = [
        Self::IoUring,
        Self::NetAdmin,
        Self::NetRaw,
        Self::Bpf,
        Self::Perf,
        Self::SysAdmin,
    ];

    pub(in crate::guest_init) const fn as_str(self) -> &'static str {
        match self {
            Self::IoUring => "io-uring",
            Self::NetAdmin => "net-admin",
            Self::NetRaw => "net-raw",
            Self::Bpf => "bpf",
            Self::Perf => "perf",
            Self::SysAdmin => "sys-admin",
        }
    }

    const fn bit(self) -> u8 {
        match self {
            Self::IoUring => 1 << 0,
            Self::NetAdmin => 1 << 1,
            Self::NetRaw => 1 << 2,
            Self::Bpf => 1 << 3,
            Self::Perf => 1 << 4,
            Self::SysAdmin => 1 << 5,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(in crate::guest_init) struct GuestPermissions(u8);

impl GuestPermissions {
    pub(in crate::guest_init) const fn contains(self, permission: GuestPermission) -> bool {
        self.0 & permission.bit() != 0
    }
}

impl FromStr for GuestPermissions {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self> {
        if value.is_empty() {
            anyhow::bail!("LOFTD_PERMISSIONS must not be empty");
        }
        let mut permissions = Self::default();
        for token in value.split(',') {
            let permission = match token {
                "io-uring" => GuestPermission::IoUring,
                "net-admin" => GuestPermission::NetAdmin,
                "net-raw" => GuestPermission::NetRaw,
                "bpf" => GuestPermission::Bpf,
                "perf" => GuestPermission::Perf,
                "sys-admin" => GuestPermission::SysAdmin,
                "" => anyhow::bail!("LOFTD_PERMISSIONS must not contain empty values"),
                other => {
                    anyhow::bail!("LOFTD_PERMISSIONS contains unsupported permission '{other}'")
                }
            };
            permissions.0 |= permission.bit();
        }
        Ok(permissions)
    }
}

impl std::fmt::Display for GuestPermissions {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut first = true;
        for permission in GuestPermission::ALL {
            if self.contains(permission) {
                if !first {
                    formatter.write_str(",")?;
                }
                formatter.write_str(permission.as_str())?;
                first = false;
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::guest_init) struct LoftdEnv {
    pub(in crate::guest_init) nix_overlay: bool,
    pub(in crate::guest_init) nix_host_overlay: bool,
    pub(in crate::guest_init) containers_storage: bool,
    pub(in crate::guest_init) container_store_backend: ContainerStoreBackend,
    pub(in crate::guest_init) use_passt: bool,
    pub(in crate::guest_init) gpu_drm: bool,
    pub(in crate::guest_init) wayland: bool,
    pub(in crate::guest_init) permissions: GuestPermissions,
    pub(in crate::guest_init) enter_as_root: bool,
    pub(in crate::guest_init) host_uid: Option<u32>,
    pub(in crate::guest_init) host_gid: Option<u32>,
    pub(in crate::guest_init) nix_disk_id: String,
    pub(in crate::guest_init) nix_disk_label: String,
    pub(in crate::guest_init) containers_disk_id: String,
    pub(in crate::guest_init) containers_disk_label: String,
}

impl LoftdEnv {
    pub(in crate::guest_init) fn from_process_env() -> Result<Self> {
        Ok(Self {
            nix_overlay: env_flag_any("LOFTD_NIX_OVERLAY", LEGACY_NIX_OVERLAY_ENV),
            nix_host_overlay: env_flag("LOFTD_NIX_HOST_OVERLAY"),
            containers_storage: env_flag_any(
                "LOFTD_CONTAINERS_STORAGE",
                LEGACY_CONTAINERS_STORAGE_ENV,
            ),
            container_store_backend: ContainerStoreBackend::from_optional_env_value(
                env::var(CONTAINERS_STORE_ENV)
                    .ok()
                    .filter(|value| !value.is_empty()),
            )?,
            use_passt: env_flag_any("LOFTD_USE_PASST", LEGACY_USE_PASST_ENV),
            gpu_drm: env_flag("LOFTD_GPU_DRM"),
            wayland: env_flag("LOFTD_WAYLAND"),
            permissions: env::var("LOFTD_PERMISSIONS")
                .ok()
                .map_or(Ok(GuestPermissions::default()), |value| value.parse())?,
            enter_as_root: env_flag_any(ENTER_AS_ROOT_ENV, LEGACY_ENTER_AS_ROOT_ENV),
            host_uid: parse_optional_u32_any("LOFTD_HOST_UID", LEGACY_HOST_UID_ENV)?,
            host_gid: parse_optional_u32_any("LOFTD_HOST_GID", LEGACY_HOST_GID_ENV)?,
            nix_disk_id: env::var("LOFTD_NIX_DISK_ID")
                .unwrap_or_else(|_| RAW_NIX_DISK_ID.to_owned()),
            nix_disk_label: env::var("LOFTD_NIX_DISK_LABEL")
                .unwrap_or_else(|_| RAW_NIX_DISK_LABEL.to_owned()),
            containers_disk_id: env::var("LOFTD_CONTAINERS_DISK_ID")
                .unwrap_or_else(|_| RAW_CONTAINER_DISK_ID.to_owned()),
            containers_disk_label: env::var("LOFTD_CONTAINERS_DISK_LABEL")
                .unwrap_or_else(|_| RAW_CONTAINER_DISK_LABEL.to_owned()),
        })
    }

    pub(in crate::guest_init) fn require_host_identity(&self) -> Result<(u32, u32)> {
        let uid = self
            .host_uid
            .ok_or_else(|| anyhow!("LOFTD_HOST_UID is required for loftd guest init"))?;
        let gid = self
            .host_gid
            .ok_or_else(|| anyhow!("LOFTD_HOST_GID is required for loftd guest init"))?;
        Ok((uid, gid))
    }
}

fn env_flag_any(primary: &str, legacy: &str) -> bool {
    env_flag(primary) || env_flag(legacy)
}

fn env_flag(name: &str) -> bool {
    env::var(name).as_deref() == Ok("1")
}

fn parse_optional_u32_any(primary: &str, legacy: &str) -> Result<Option<u32>> {
    parse_optional_u32(primary)?.map_or_else(|| parse_optional_u32(legacy), |value| Ok(Some(value)))
}

fn parse_optional_u32(name: &str) -> Result<Option<u32>> {
    match env::var(name) {
        Ok(value) if !value.is_empty() => {
            Ok(Some(value.parse().with_context(|| {
                format!("invalid numeric value in {name}")
            })?))
        }
        _ => Ok(None),
    }
}

#[cfg(test)]
#[path = "env_tests.rs"]
mod tests;
