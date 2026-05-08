use std::path::{Path, PathBuf};

use crate::{
    HOST_NIX_MERGED_DIR, HOST_NIX_SIDECAR_STATE_FILE, HOST_NIX_UPPER_DIR, HOST_NIX_WORK_DIR,
};

#[derive(Debug, Clone)]
pub struct SidecarPaths {
    pub upper_dir: PathBuf,
    pub work_dir: PathBuf,
    pub merged_dir: PathBuf,
    pub state_file: PathBuf,
}

#[derive(Debug, Clone)]
pub struct SidecarState {
    pub image: String,
    pub image_id: String,
    pub image_mount_path: PathBuf,
    pub sidecar_name: String,
    pub mount_mode: PodmanImageMountMode,
    pub proxy_port: Option<u16>,
    pub native_config: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PodmanImageMountMode {
    Direct,
    Unshare,
}

#[derive(Debug, Clone, Copy)]
pub(in crate::runtime::container) struct SidecarDaemonRuntimeSpec {
    pub socket_health_probe: SidecarSocketHealthProbe,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::runtime::container) enum SidecarSocketHealthProbe {
    Enabled,
    Disabled,
}

impl SidecarSocketHealthProbe {
    pub fn enabled(self) -> bool {
        self == Self::Enabled
    }
}

impl SidecarPaths {
    pub fn new(state_root: &Path) -> Self {
        Self {
            upper_dir: state_root.join(HOST_NIX_UPPER_DIR),
            work_dir: state_root.join(HOST_NIX_WORK_DIR),
            merged_dir: state_root.join(HOST_NIX_MERGED_DIR),
            state_file: state_root.join(HOST_NIX_SIDECAR_STATE_FILE),
        }
    }
}

impl SidecarState {
    pub fn matches_identity(&self, image: &str, image_id: &str, sidecar_name: &str) -> bool {
        self.image == image && self.image_id == image_id && self.sidecar_name == sidecar_name
    }

    pub fn matches(&self, image: &str, image_id: &str, sidecar_name: &str) -> bool {
        self.matches_identity(image, image_id, sidecar_name) && self.native_config
    }
}

impl PodmanImageMountMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Direct => "podman image mount",
            Self::Unshare => "podman unshare podman image mount",
        }
    }
}
