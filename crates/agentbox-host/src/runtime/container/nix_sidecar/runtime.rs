use std::path::PathBuf;

use crate::runtime::container::nix_sidecar::PodmanImageMountMode;

#[derive(Debug, Clone)]
pub(in crate::runtime::container) struct SidecarNixRuntime {
    pub(in crate::runtime::container) merged_dir: PathBuf,
    pub(in crate::runtime::container) sidecar_name: String,
    pub(in crate::runtime::container) proxy_port: u16,
    pub(in crate::runtime::container) mount_mode: PodmanImageMountMode,
}
