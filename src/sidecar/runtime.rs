use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct SidecarNixRuntime {
    pub lease_file: PathBuf,
    pub state_root: PathBuf,
    pub generation: String,
    pub merged_dir: PathBuf,
    pub sidecar_name: String,
}
