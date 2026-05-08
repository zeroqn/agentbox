use anyhow::{anyhow, Context, Result};
use std::fs;
use std::path::Path;
use std::process::Stdio;

use crate::container::nix_sidecar::health;
use crate::container::nix_sidecar::image_mount::{
    inspect_image_id, mount_image_with_lowerdir, unmount_image,
};
use crate::container::nix_sidecar::name;
use crate::container::nix_sidecar::overlay::{
    cleanup_merged_mount, cleanup_merged_mount_all_namespaces, mount_fuse_overlayfs,
};
use crate::container::nix_sidecar::reuse::{
    reject_active_legacy_sidecar_config, should_reuse_previous_sidecar,
};
use crate::container::nix_sidecar::runtime::SidecarNixRuntime;
use crate::container::nix_sidecar::sidecar_podman::container::{
    build_merged_mount_arg, build_sidecar_podman_args, cleanup_sidecar_container,
    SIDECAR_ENTRYPOINT,
};
use crate::container::nix_sidecar::sidecar_podman::proxy::{
    resolve_runtime_proxy_port_or_default, resolve_sidecar_proxy_port,
};
use crate::container::nix_sidecar::sidecar_podman::requirements::ensure_command_available;
use crate::container::nix_sidecar::sidecar_podman::task_probe::sidecar_has_running_task_containers;
use crate::container::nix_sidecar::state;
use crate::container::nix_sidecar::types::{SidecarDaemonRuntimeSpec, SidecarPaths, SidecarState};
use crate::podman::command::run_podman;

pub(in crate::container) fn prepare_sidecar_nix_runtime(
    cwd: &Path,
    state_root: &Path,
    image: &str,
    runtime_spec: SidecarDaemonRuntimeSpec,
) -> Result<SidecarNixRuntime> {
    ensure_command_available("fuse-overlayfs", "required for sidecar mode")?;

    let paths = SidecarPaths::new(state_root);
    fs::create_dir_all(state_root)
        .with_context(|| format!("failed to create '{}'", state_root.display()))?;
    create_sidecar_dirs(&paths)?;

    let image_id = inspect_image_id(image)?;
    let sidecar_name = name::derive_sidecar_name(cwd, &image_id);
    let previous_state = state::read_sidecar_state(&paths)?;

    if let Some(state) = previous_state.as_ref() {
        let reusable_config_matches = state.matches(image, &image_id, &sidecar_name);
        if should_reuse_previous_sidecar(
            state,
            &paths,
            image,
            reusable_config_matches,
            runtime_spec.socket_health_probe,
        )? {
            let proxy_port =
                resolve_runtime_proxy_port_or_default(resolve_sidecar_proxy_port(&sidecar_name));
            return Ok(SidecarNixRuntime {
                merged_dir: paths.merged_dir,
                sidecar_name: sidecar_name.clone(),
                proxy_port,
                mount_mode: state.mount_mode,
            });
        }
    }

    reject_active_legacy_sidecar_config(previous_state.as_ref(), image, &image_id, &sidecar_name)?;

    recreate_sidecar_stack(
        &paths,
        image,
        &image_id,
        &sidecar_name,
        previous_state.as_ref(),
        runtime_spec,
    )
}

pub(in crate::container) fn cleanup_idle_sidecar(sidecar: &SidecarNixRuntime) -> Result<()> {
    if preserve_idle_sidecar(sidecar_has_running_task_containers(&sidecar.sidecar_name)?) {
        return Ok(());
    }

    cleanup_sidecar_container(&sidecar.sidecar_name)?;
    cleanup_merged_mount(&sidecar.merged_dir, sidecar.mount_mode)
}

fn create_sidecar_dirs(paths: &SidecarPaths) -> Result<()> {
    for dir in [&paths.upper_dir, &paths.work_dir, &paths.merged_dir] {
        fs::create_dir_all(dir).with_context(|| format!("failed to create '{}'", dir.display()))?;
    }
    Ok(())
}

fn preserve_idle_sidecar(has_running_task_containers: bool) -> bool {
    has_running_task_containers
}

fn recreate_sidecar_stack(
    paths: &SidecarPaths,
    image: &str,
    image_id: &str,
    sidecar_name: &str,
    previous_state: Option<&SidecarState>,
    runtime_spec: SidecarDaemonRuntimeSpec,
) -> Result<SidecarNixRuntime> {
    cleanup_previous_stack(paths, sidecar_name, previous_state)?;

    let (image_mount_path, lowerdir, mount_mode) = mount_image_with_lowerdir(image)?;

    mount_fuse_overlayfs(
        &lowerdir,
        &paths.upper_dir,
        &paths.work_dir,
        &paths.merged_dir,
        mount_mode,
    )?;

    let merged_mount_arg = build_merged_mount_arg(&paths.merged_dir)?;
    let sidecar_args = build_sidecar_podman_args(image, sidecar_name, &merged_mount_arg)?;
    let status = run_podman(
        sidecar_args,
        Stdio::null(),
        Stdio::null(),
        Stdio::inherit(),
        "failed to start nix-daemon sidecar",
    )?;
    if !status.success() {
        return Err(anyhow!(
            "nix-daemon sidecar '{}' failed to start; ensure image '{}' contains sidecar entrypoint '{}' and rebuild/load the image if needed",
            sidecar_name,
            image,
            SIDECAR_ENTRYPOINT
        ));
    }

    if runtime_spec.socket_health_probe.enabled() {
        health::wait_for_socket_health(image, sidecar_name, &paths.merged_dir, mount_mode)?;
    }

    let proxy_port =
        resolve_runtime_proxy_port_or_default(resolve_sidecar_proxy_port(sidecar_name));

    let new_state = SidecarState {
        image: image.to_owned(),
        image_id: image_id.to_owned(),
        image_mount_path,
        sidecar_name: sidecar_name.to_owned(),
        mount_mode,
        proxy_port: Some(proxy_port),
        native_config: true,
    };
    state::write_sidecar_state(paths, &new_state)?;

    Ok(SidecarNixRuntime {
        merged_dir: paths.merged_dir.clone(),
        sidecar_name: sidecar_name.to_owned(),
        proxy_port,
        mount_mode,
    })
}

fn cleanup_previous_stack(
    paths: &SidecarPaths,
    sidecar_name: &str,
    previous_state: Option<&SidecarState>,
) -> Result<()> {
    if let Some(state) = previous_state {
        cleanup_sidecar_container(&state.sidecar_name)?;
        cleanup_merged_mount(&paths.merged_dir, state.mount_mode)?;
        unmount_image(&state.image)?;
        return Ok(());
    }

    cleanup_sidecar_container(sidecar_name)?;
    cleanup_merged_mount_all_namespaces(&paths.merged_dir)
}

#[cfg(test)]
mod tests;
