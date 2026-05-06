use std::path::PathBuf;

use super::PodmanImageMountMode;

#[derive(Debug, Clone)]
pub(crate) struct SidecarNixRuntime {
    pub(crate) merged_dir: PathBuf,
    pub(crate) sidecar_name: String,
    pub(crate) proxy_port: u16,
    pub(crate) mount_mode: PodmanImageMountMode,
}
