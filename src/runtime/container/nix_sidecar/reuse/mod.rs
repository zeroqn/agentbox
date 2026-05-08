use anyhow::Result;

use crate::runtime::container::nix_sidecar::health;
use crate::runtime::container::nix_sidecar::probe::is_container_running;
use crate::runtime::container::nix_sidecar::sidecar_podman::task_probe::sidecar_has_running_task_containers;
use crate::runtime::container::nix_sidecar::types::{
    SidecarPaths, SidecarSocketHealthProbe, SidecarState,
};

pub(in crate::runtime::container::nix_sidecar) fn should_reuse_previous_sidecar(
    state: &SidecarState,
    paths: &SidecarPaths,
    image: &str,
    reusable_config_matches: bool,
    socket_health_probe: SidecarSocketHealthProbe,
) -> Result<bool> {
    if !reusable_config_matches {
        return Ok(false);
    }

    let sidecar_running = is_container_running(&state.sidecar_name);
    let protected_same_repo_reuse = protected_same_repo_reuse_applies(
        reusable_config_matches,
        sidecar_running,
        sidecar_has_running_task_containers(&state.sidecar_name),
    );
    if protected_same_repo_reuse {
        return Ok(true);
    }

    Ok(fallback_health_gated_reuse_applies(
        reusable_config_matches,
        protected_same_repo_reuse,
        sidecar_stack_is_reusable(state, paths, image, socket_health_probe)?,
    ))
}

pub(in crate::runtime::container::nix_sidecar) fn reject_active_legacy_sidecar_config(
    state: Option<&SidecarState>,
    image: &str,
    image_id: &str,
    sidecar_name: &str,
) -> Result<()> {
    let Some(state) = state else {
        return Ok(());
    };

    if active_legacy_sidecar_config_applies(
        state,
        image,
        image_id,
        sidecar_name,
        sidecar_has_running_task_containers(&state.sidecar_name)?,
    ) {
        anyhow::bail!(
            "nix-daemon sidecar '{}' was started by a legacy non-container configuration and matching task containers are still active; wait for those tasks to exit before recreating the container-mode sidecar",
            state.sidecar_name
        );
    }

    Ok(())
}

fn sidecar_stack_is_reusable(
    state: &SidecarState,
    paths: &SidecarPaths,
    image: &str,
    socket_health_probe: SidecarSocketHealthProbe,
) -> Result<bool> {
    if socket_health_probe.enabled() {
        health::sidecar_stack_is_healthy(state, paths, image)
    } else {
        health::sidecar_stack_is_present(state, paths)
    }
}

fn active_legacy_sidecar_config_applies(
    state: &SidecarState,
    image: &str,
    image_id: &str,
    sidecar_name: &str,
    running_task_containers: bool,
) -> bool {
    state.matches_identity(image, image_id, sidecar_name)
        && !state.native_config
        && running_task_containers
}

fn protected_same_repo_reuse_applies(
    identity_matches: bool,
    sidecar_running: bool,
    running_task_probe: Result<bool>,
) -> bool {
    if !identity_matches || !sidecar_running {
        return false;
    }

    matches!(running_task_probe, Ok(true))
}

fn fallback_health_gated_reuse_applies(
    identity_matches: bool,
    protected_same_repo_reuse: bool,
    sidecar_stack_is_healthy: bool,
) -> bool {
    !protected_same_repo_reuse && identity_matches && sidecar_stack_is_healthy
}

#[cfg(test)]
mod tests;
