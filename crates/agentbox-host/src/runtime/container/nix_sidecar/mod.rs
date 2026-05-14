mod health;
mod image_mount;
mod lifecycle;
mod lowerdir;
mod name;
mod overlay;
mod podman;
mod probe;
mod reuse;
mod runtime;
mod sidecar_podman;
mod state;
mod types;

pub(in crate::runtime::container) use lifecycle::{
    cleanup_idle_sidecar, prepare_sidecar_nix_runtime,
};
pub(in crate::runtime::container) use podman::append_task_args as append_task_sidecar_nix_args;
#[cfg(test)]
pub(crate) use podman::SIDECAR_NIX_OWNER;
pub(in crate::runtime::container) use runtime::SidecarNixRuntime;
pub(in crate::runtime::container) use types::{
    PodmanImageMountMode, SidecarDaemonRuntimeSpec, SidecarSocketHealthProbe,
};

#[cfg(test)]
pub use lowerdir::resolve_sidecar_lowerdir;
#[cfg(test)]
pub use types::{SidecarPaths, SidecarState};

#[cfg(test)]
mod tests;
