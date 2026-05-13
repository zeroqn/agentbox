use anyhow::Result;

use crate::podman::command::run_podman_output;
use crate::{TASK_CONTAINER_ROLE_LABEL, TASK_CONTAINER_ROLE_VALUE, TASK_CONTAINER_SIDECAR_LABEL};

pub(in crate::runtime::container::nix_sidecar) fn build_sidecar_task_probe_args(
    sidecar_name: &str,
) -> Vec<String> {
    vec![
        "ps".to_owned(),
        "--filter".to_owned(),
        format!("label={TASK_CONTAINER_ROLE_LABEL}={TASK_CONTAINER_ROLE_VALUE}"),
        "--filter".to_owned(),
        format!("label={TASK_CONTAINER_SIDECAR_LABEL}={sidecar_name}"),
        "--format".to_owned(),
        "{{.ID}}".to_owned(),
    ]
}

pub(in crate::runtime::container::nix_sidecar) fn sidecar_has_running_task_containers(
    sidecar_name: &str,
) -> Result<bool> {
    let args = build_sidecar_task_probe_args(sidecar_name);
    let output = run_podman_output(
        args,
        "failed to inspect running task containers for sidecar cleanup",
    )?;

    Ok(output.lines().any(|line| !line.trim().is_empty()))
}
