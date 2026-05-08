use std::path::PathBuf;

use crate::container::nix_sidecar::PodmanImageMountMode;

#[derive(Debug, Clone)]
pub(in crate::container) struct SidecarNixRuntime {
    pub(in crate::container) merged_dir: PathBuf,
    pub(in crate::container) sidecar_name: String,
    pub(in crate::container) proxy_port: u16,
    pub(in crate::container) mount_mode: PodmanImageMountMode,
}
