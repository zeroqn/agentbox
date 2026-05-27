pub(crate) mod image_cache;
pub(crate) mod storage;

use anyhow::Result;
use std::process::ExitCode;

use crate::cli::{CommonOptions, MicrovmOptions};
use crate::naming::derive_task_container_name;
use crate::state::resolve_state_layout;

use self::image_cache::{HostBuildahRunner, ImageCache, ImageReference};
use self::storage::{HostStorageProbe, StorageManager};

pub(crate) fn run(common: CommonOptions, options: MicrovmOptions) -> Result<ExitCode> {
    let cwd = std::env::current_dir()?;
    let state_layout = resolve_state_layout(&cwd)?;
    let task_id = derive_task_container_name(&cwd);
    run_with_layout(common, options, &state_layout, &task_id)
}

fn run_with_layout(
    common: CommonOptions,
    options: MicrovmOptions,
    state_layout: &crate::state::StateLayout,
    task_id: &str,
) -> Result<ExitCode> {
    if common.pull_latest {
        anyhow::bail!(pull_latest_not_supported_message());
    }

    let reference = ImageReference::from_cli(common.image.as_deref());
    let cache = ImageCache::new(state_layout.microvm_image_cache_dir());
    let entry = cache.ensure(reference, &HostBuildahRunner)?;
    let backend = StorageManager::select_backend(options.storage, &HostStorageProbe)?;
    let storage = StorageManager::new(state_layout.root_dir().to_path_buf());
    let handle = storage.materialize(&entry, backend, task_id, options.preserve_debug)?;
    let root = handle.root.clone();
    let cleanup_result = handle.cleanup()?;
    anyhow::bail!(
        "{} (prepared task rootfs at '{}'; cleanup: {cleanup_result:?})",
        boot_pending_message(),
        root.display()
    )
}

pub(crate) fn boot_pending_message() -> &'static str {
    "experimental microvm image-cache/task-rootfs preparation succeeded; direct libkrun boot is not implemented yet"
}

pub(crate) fn pull_latest_not_supported_message() -> &'static str {
    "agentbox --pull-latest microvm is not supported yet; experimental microvm image refresh must use a future Buildah-backed path, not Podman"
}
