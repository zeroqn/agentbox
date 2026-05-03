use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct SidecarNixRuntime {
    pub merged_dir: PathBuf,
    pub sidecar_name: String,
    pub proxy_port: u16,
}
