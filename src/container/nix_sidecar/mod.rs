mod health;
mod image_mount;
mod lifecycle;
mod lowerdir;
mod name;
mod overlay;
mod probe;
mod reuse;
mod runtime;
mod sidecar_podman;
mod state;
mod types;

pub(in crate::container) use lifecycle::{cleanup_idle_sidecar, prepare_sidecar_nix_runtime};
pub(in crate::container) use runtime::SidecarNixRuntime;
pub(in crate::container) use types::{
    PodmanImageMountMode, SidecarDaemonRuntimeSpec, SidecarSocketHealthProbe,
};

#[cfg(test)]
pub use lowerdir::resolve_sidecar_lowerdir;
#[cfg(test)]
pub use types::{SidecarPaths, SidecarState};

#[cfg(test)]
mod tests;
