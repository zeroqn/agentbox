use anyhow::{anyhow, Context, Result};
use std::fs;
use std::os::unix::fs::FileTypeExt;
use std::path::Path;
use std::thread;
use std::time::Duration;

use crate::mounts::format::format_mount_arg_with_options;
use crate::podman::command::{run_podman_capture, run_podman_output};
use crate::{
    CONTAINER_NIX_DIR, NIX_REMOTE_SOCKET, SIDECAR_HEALTH_ATTEMPTS, SIDECAR_HEALTH_DELAY_MS,
    SIDECAR_LOG_TAIL_LINES, SIDECAR_SOCKET_PATH,
};

use super::overlay::{cleanup_merged_mount, path_is_mounted};
use super::{cleanup_sidecar_container, resolve_sidecar_lowerdir_for_mode, SidecarState};

#[derive(Debug, Clone)]
struct SidecarStartupCleanupOutcome {
    summary: String,
    manual_merged_cleanup_required: bool,
}

#[derive(Debug, Clone, Default)]
struct SidecarStartupDiagnostics {
    lowerdir_resolution_error: Option<String>,
    sidecar_logs: Option<String>,
    sidecar_logs_error: Option<String>,
    socket_probe_failure: Option<String>,
    sidecar_state: Option<String>,
    host_socket_exists: Option<bool>,
    merged_mount_active: Option<bool>,
    sidecar_running: Option<bool>,
}

pub fn sidecar_stack_is_healthy(state: &SidecarState, image: &str) -> Result<bool> {
    if resolve_sidecar_lowerdir_for_mode(&state.image_mount_path, state.mount_mode).is_err() {
        return Ok(false);
    }

    if !path_is_mounted(&state.merged_dir)? {
        return Ok(false);
    }

    if !is_container_running(&state.sidecar_name) {
        return Ok(false);
    }

    if daemon_socket_probe_failure(image, &state.merged_dir)?.is_some() {
        return Ok(false);
    }

    Ok(true)
}

pub fn build_current_generation_unhealthy_error(
    state: &SidecarState,
    image: &str,
    live_references: bool,
) -> String {
    let diagnostics = collect_sidecar_diagnostics(state, image);
    let reference_summary = if live_references {
        "live task references still exist"
    } else {
        "no live task references remain"
    };
    let mut message = format!(
        "current nix sidecar generation '{}' is unhealthy for sidecar '{}' and {}; refusing to reuse it",
        state.generation, state.sidecar_name, reference_summary
    );

    if live_references {
        message
            .push_str(" and refusing to create a replacement generation while it is still in use.");
    } else {
        message.push('.');
    }

    append_sidecar_diagnostics(&mut message, &diagnostics);
    message
}

pub fn wait_for_socket_health(image: &str, sidecar_name: &str, merged_dir: &Path) -> Result<()> {
    let mut last_probe_failure = None;
    let mut last_host_socket_exists = None;
    for _attempt in 0..SIDECAR_HEALTH_ATTEMPTS {
        last_host_socket_exists = Some(daemon_socket_exists(merged_dir)?);
        match daemon_socket_probe_failure(image, merged_dir)? {
            None => return Ok(()),
            Some(probe_failure) => last_probe_failure = Some(probe_failure),
        }
        thread::sleep(Duration::from_millis(SIDECAR_HEALTH_DELAY_MS));
    }

    let (sidecar_logs, sidecar_logs_error) = match read_sidecar_logs(sidecar_name) {
        Ok(logs) => (Some(logs), None),
        Err(err) => (None, Some(err.to_string())),
    };
    let diagnostics = SidecarStartupDiagnostics {
        lowerdir_resolution_error: None,
        sidecar_logs,
        sidecar_logs_error,
        socket_probe_failure: last_probe_failure.or_else(|| {
            daemon_socket_probe_failure(image, merged_dir)
                .ok()
                .flatten()
        }),
        sidecar_state: inspect_sidecar_container_state(sidecar_name).ok(),
        host_socket_exists: last_host_socket_exists,
        merged_mount_active: Some(path_is_mounted(merged_dir).unwrap_or(false)),
        sidecar_running: Some(is_container_running(sidecar_name)),
    };
    let cleanup_outcome = cleanup_failed_sidecar_startup(sidecar_name, merged_dir);
    Err(anyhow!(
        "{}",
        build_sidecar_socket_timeout_error(
            sidecar_name,
            merged_dir,
            &cleanup_outcome,
            &diagnostics
        )
    ))
}

fn is_container_running(container_name: &str) -> bool {
    let args = vec![
        "container".to_owned(),
        "inspect".to_owned(),
        "--format".to_owned(),
        "{{.State.Running}}".to_owned(),
        container_name.to_owned(),
    ];

    match run_podman_output(args, "failed to inspect sidecar container") {
        Ok(output) => output.trim() == "true",
        Err(_) => false,
    }
}

fn daemon_socket_exists(merged_dir: &Path) -> Result<bool> {
    let socket_path = merged_dir
        .join("var")
        .join("nix")
        .join("daemon-socket")
        .join("socket");

    if !socket_path.exists() {
        return Ok(false);
    }

    let metadata = fs::metadata(&socket_path)
        .with_context(|| format!("failed to inspect '{}'", socket_path.display()))?;
    Ok(metadata.file_type().is_socket())
}

fn daemon_socket_probe_failure(image: &str, merged_dir: &Path) -> Result<Option<String>> {
    let merged_mount_arg =
        format_mount_arg_with_options(merged_dir, CONTAINER_NIX_DIR, Some("ro"))?;
    let args = build_socket_ping_podman_args(image, &merged_mount_arg);
    let output = run_podman_capture(args, "failed to probe nix-daemon socket")?;
    if output.status.success() {
        return Ok(None);
    }

    let status = output
        .status
        .code()
        .map_or_else(|| "signal".to_owned(), |code| code.to_string());
    let stderr = summarize_command_output(&output.stderr);
    let stdout = summarize_command_output(&output.stdout);
    let mut details = vec![format!("probe exited with status {status}")];
    if !stderr.is_empty() {
        details.push(format!("stderr: {stderr}"));
    }
    if !stdout.is_empty() {
        details.push(format!("stdout: {stdout}"));
    }
    Ok(Some(details.join("; ")))
}

fn summarize_command_output(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn inspect_sidecar_container_state(sidecar_name: &str) -> Result<String> {
    let args = vec![
        "container".to_owned(),
        "inspect".to_owned(),
        "--format".to_owned(),
        "running={{.State.Running}} status={{.State.Status}} exit_code={{.State.ExitCode}} error={{.State.Error}} oom_killed={{.State.OOMKilled}}".to_owned(),
        sidecar_name.to_owned(),
    ];
    let output = run_podman_output(args, "failed to inspect nix-daemon sidecar state")?;
    let summary = output.trim();
    if summary.is_empty() {
        return Err(anyhow!(
            "nix-daemon sidecar '{}' inspect returned empty output",
            sidecar_name
        ));
    }
    Ok(summary.to_owned())
}

fn read_sidecar_logs(sidecar_name: &str) -> Result<String> {
    let args = vec![
        "logs".to_owned(),
        "--tail".to_owned(),
        SIDECAR_LOG_TAIL_LINES.to_string(),
        sidecar_name.to_owned(),
    ];
    let logs = run_podman_output(args, "failed to read nix-daemon sidecar logs")?;
    let trimmed = logs.trim();
    if trimmed.is_empty() {
        return Err(anyhow!(
            "nix-daemon sidecar '{}' emitted no logs",
            sidecar_name
        ));
    }
    Ok(trimmed.to_owned())
}

fn cleanup_failed_sidecar_startup(
    sidecar_name: &str,
    merged_dir: &Path,
) -> SidecarStartupCleanupOutcome {
    let mut summary = Vec::new();

    match cleanup_sidecar_container(sidecar_name) {
        Ok(()) => summary.push(format!(
            "removed sidecar '{}' (or it was already absent)",
            sidecar_name
        )),
        Err(err) => summary.push(format!(
            "failed to remove sidecar '{}': {err:#}",
            sidecar_name
        )),
    }

    let manual_merged_cleanup_required = match cleanup_merged_mount(merged_dir) {
        Ok(()) => {
            summary.push(format!("cleaned merged mount '{}'", merged_dir.display()));
            false
        }
        Err(err) => {
            summary.push(format!(
                "failed to clean merged mount '{}': {err:#}",
                merged_dir.display()
            ));
            true
        }
    };

    SidecarStartupCleanupOutcome {
        summary: summary.join("; "),
        manual_merged_cleanup_required,
    }
}

fn build_sidecar_socket_timeout_error(
    sidecar_name: &str,
    merged_dir: &Path,
    cleanup_outcome: &SidecarStartupCleanupOutcome,
    diagnostics: &SidecarStartupDiagnostics,
) -> String {
    let mut message = format!(
        "nix-daemon socket '{}' was not connectable after startup for sidecar '{}'; {}.",
        SIDECAR_SOCKET_PATH, sidecar_name, cleanup_outcome.summary
    );

    if cleanup_outcome.manual_merged_cleanup_required {
        message.push_str(&format!(
            " The merged mount '{}' could not be cleaned automatically; remove it before retrying.",
            merged_dir.display()
        ));
    } else {
        message.push_str(&format!(
            " Automatic cleanup completed; retrying should not require manual '{}' removal.",
            merged_dir.display()
        ));
    }

    append_sidecar_diagnostics(&mut message, diagnostics);

    message
}

fn append_sidecar_diagnostics(message: &mut String, diagnostics: &SidecarStartupDiagnostics) {
    if let Some(error) = diagnostics
        .lowerdir_resolution_error
        .as_deref()
        .map(str::trim)
        .filter(|error| !error.is_empty())
    {
        message.push_str("\nlowerdir resolution error: ");
        message.push_str(error);
    }

    if let Some(active) = diagnostics.merged_mount_active {
        message.push_str("\nmerged mount active: ");
        message.push_str(if active { "yes" } else { "no" });
    }

    if let Some(running) = diagnostics.sidecar_running {
        message.push_str("\nsidecar running: ");
        message.push_str(if running { "yes" } else { "no" });
    }

    if let Some(logs) = diagnostics
        .sidecar_logs
        .as_deref()
        .map(str::trim)
        .filter(|logs| !logs.is_empty())
    {
        message.push_str("\nrecent sidecar logs:\n");
        message.push_str(logs);
    } else {
        message.push_str("\nsidecar logs unavailable");
        if let Some(err) = diagnostics
            .sidecar_logs_error
            .as_deref()
            .map(str::trim)
            .filter(|err| !err.is_empty())
        {
            message.push_str(&format!(" ({err})"));
        }
        message.push_str(
            "; this usually means the sidecar terminated before logs could be collected.",
        );
    }

    if let Some(state) = diagnostics
        .sidecar_state
        .as_deref()
        .map(str::trim)
        .filter(|state| !state.is_empty())
    {
        message.push_str("\nsidecar state: ");
        message.push_str(state);
    }

    if let Some(probe) = diagnostics
        .socket_probe_failure
        .as_deref()
        .map(str::trim)
        .filter(|probe| !probe.is_empty())
    {
        message.push_str("\nsocket probe failure: ");
        message.push_str(probe);
    }

    if let Some(exists) = diagnostics.host_socket_exists {
        message.push_str("\nhost socket path exists: ");
        message.push_str(if exists { "yes" } else { "no" });
    }
}

fn collect_sidecar_diagnostics(state: &SidecarState, image: &str) -> SidecarStartupDiagnostics {
    let lowerdir_resolution_error =
        resolve_sidecar_lowerdir_for_mode(&state.image_mount_path, state.mount_mode)
            .err()
            .map(|err| err.to_string());
    let merged_mount_active = path_is_mounted(&state.merged_dir).ok();
    let sidecar_running = Some(is_container_running(&state.sidecar_name));
    let socket_probe_failure = daemon_socket_probe_failure(image, &state.merged_dir)
        .ok()
        .flatten();
    let host_socket_exists = daemon_socket_exists(&state.merged_dir).ok();
    let (sidecar_logs, sidecar_logs_error) = match read_sidecar_logs(&state.sidecar_name) {
        Ok(logs) => (Some(logs), None),
        Err(err) => (None, Some(err.to_string())),
    };

    SidecarStartupDiagnostics {
        lowerdir_resolution_error,
        sidecar_logs,
        sidecar_logs_error,
        socket_probe_failure,
        sidecar_state: inspect_sidecar_container_state(&state.sidecar_name).ok(),
        host_socket_exists,
        merged_mount_active,
        sidecar_running,
    }
}

fn build_socket_ping_podman_args(image: &str, merged_mount: &str) -> Vec<String> {
    vec![
        "run".to_owned(),
        "--rm".to_owned(),
        "--userns".to_owned(),
        "keep-id".to_owned(),
        "--volume".to_owned(),
        merged_mount.to_owned(),
        image.to_owned(),
        "bash".to_owned(),
        "-lc".to_owned(),
        format!("nix store ping --store {NIX_REMOTE_SOCKET}"),
    ]
}

#[cfg(test)]
mod tests;
