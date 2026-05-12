mod nix_sidecar;
mod task;

use anyhow::{Context, Result};
use std::env;
use std::process::{ExitCode, Stdio};

use crate::cli::{env_flag_enabled, resolve_image, resolve_nix_sidecar_enabled, Cli};
use crate::mounts::format::format_mount_arg;
use crate::mounts::{
    prepare_host_codex_mount, prepare_project_cargo_mount, prepare_shared_sccache_mount,
};
use crate::podman::command::run_podman;
use crate::runtime::container::nix_sidecar::{
    cleanup_idle_sidecar, prepare_sidecar_nix_runtime, SidecarDaemonRuntimeSpec,
    SidecarSocketHealthProbe,
};
use crate::state::resolve_state_layout;
use crate::{
    derive_task_container_name, derive_task_hostname, CONTAINER_WORKDIR,
    DEFAULT_NIX_SIDECAR_ENABLED,
};

use task::{build_container_task_podman_args, ContainerTaskPodmanSpec};

pub(crate) fn run(cli: Cli) -> Result<ExitCode> {
    let env_sidecar_enabled =
        env_flag_enabled("AGENTBOX_NIX_SIDECAR", DEFAULT_NIX_SIDECAR_ENABLED)?;
    let nix_sidecar_enabled = resolve_nix_sidecar_enabled(&cli, env_sidecar_enabled);
    validate_sidecar_mode(cli.sidecar_only, nix_sidecar_enabled)?;

    let cwd = env::current_dir()
        .context("failed to resolve current directory")?
        .canonicalize()
        .context("failed to canonicalize current directory")?;
    let image = resolve_image(cli.image.as_deref(), cli.pull_latest)?;
    let state_layout = resolve_state_layout(&cwd)?;

    if !should_launch_task_container(cli.sidecar_only) {
        let sidecar = prepare_sidecar_nix_runtime(
            &cwd,
            state_layout.root_dir(),
            &image,
            SidecarDaemonRuntimeSpec {
                socket_health_probe: sidecar_socket_health_probe(cli.sidecar_only),
            },
        )?;

        println!(
            "agentbox: sidecar '{}' is started or reused on host port {}; leaving it running for inspection",
            sidecar.sidecar_name, sidecar.proxy_port
        );
        return Ok(ExitCode::SUCCESS);
    }

    let task_container_name = derive_task_container_name(&cwd);
    let task_hostname = derive_task_hostname(&cwd);
    let workspace_mount = format_mount_arg(&cwd, CONTAINER_WORKDIR)?;
    let codex_mount = prepare_host_codex_mount()?;
    let cargo_mount = prepare_project_cargo_mount(state_layout.root_dir())?;
    let sccache_mount = prepare_shared_sccache_mount(&state_layout.sccache_dir())?;

    let nix_runtime = prepare_sidecar_nix_runtime(
        &cwd,
        state_layout.root_dir(),
        &image,
        SidecarDaemonRuntimeSpec {
            socket_health_probe: sidecar_socket_health_probe(cli.sidecar_only),
        },
    )?;

    let status = run_podman(
        build_container_task_podman_args(ContainerTaskPodmanSpec {
            image: &image,
            container_name: &task_container_name,
            hostname: &task_hostname,
            workspace_mount: &workspace_mount,
            codex_mount: &codex_mount,
            cargo_mount: &cargo_mount,
            sccache_mount: &sccache_mount,
            nix_runtime: &nix_runtime,
        })?,
        Stdio::inherit(),
        Stdio::inherit(),
        Stdio::inherit(),
        "failed to start podman",
    )?;

    if should_cleanup_idle_sidecar_after_run(cli.sidecar_only) {
        if let Err(err) = cleanup_idle_sidecar(&nix_runtime) {
            eprintln!(
                "agentbox: warning: failed to cleanup idle sidecar '{}': {err:#}",
                nix_runtime.sidecar_name
            );
        }
    }

    let code = status.code().unwrap_or(1);
    Ok(ExitCode::from(u8::try_from(code).unwrap_or(1)))
}

fn validate_sidecar_mode(sidecar_only: bool, nix_sidecar_enabled: bool) -> Result<()> {
    if nix_sidecar_enabled {
        return Ok(());
    }

    if sidecar_only {
        anyhow::bail!(
            "--sidecar-only requires nix sidecar mode; remove --disable-nix-sidecar and do not set AGENTBOX_NIX_SIDECAR=0"
        );
    }

    anyhow::bail!(
        "nix sidecar mode is required; seeded nix fallback has been removed, so remove --disable-nix-sidecar and do not set AGENTBOX_NIX_SIDECAR=0"
    );
}

fn should_launch_task_container(sidecar_only: bool) -> bool {
    !sidecar_only
}

fn should_cleanup_idle_sidecar_after_run(sidecar_only: bool) -> bool {
    !sidecar_only
}

fn sidecar_socket_health_probe(sidecar_only: bool) -> SidecarSocketHealthProbe {
    if sidecar_only {
        SidecarSocketHealthProbe::Disabled
    } else {
        SidecarSocketHealthProbe::Enabled
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sidecar_only_mode_requires_sidecar_nix_runtime() {
        let err = validate_sidecar_mode(true, false)
            .expect_err("sidecar-only without sidecar should fail");

        let message = err.to_string();
        assert!(message.contains("--sidecar-only requires nix sidecar mode"));
        assert!(message.contains("--disable-nix-sidecar"));
        assert!(message.contains("AGENTBOX_NIX_SIDECAR=0"));
    }

    #[test]
    fn normal_container_mode_rejects_disabled_sidecar_without_seeded_fallback() {
        let err = validate_sidecar_mode(false, false)
            .expect_err("disabled sidecar should fail because seeded was removed");

        let message = err.to_string();
        assert!(message.contains("nix sidecar mode is required"));
        assert!(message.contains("seeded nix fallback has been removed"));
    }

    #[test]
    fn sidecar_mode_accepts_enabled_sidecar() {
        validate_sidecar_mode(true, true).expect("sidecar-only with sidecar should be valid");
        validate_sidecar_mode(false, true).expect("normal run with sidecar should be valid");
    }

    #[test]
    fn sidecar_only_branch_skips_task_launch_and_idle_cleanup() {
        assert!(!should_launch_task_container(true));
        assert!(!should_cleanup_idle_sidecar_after_run(true));
        assert!(should_launch_task_container(false));
        assert!(should_cleanup_idle_sidecar_after_run(false));
    }

    #[test]
    fn sidecar_only_branch_disables_socket_health_probe() {
        assert_eq!(
            sidecar_socket_health_probe(true),
            SidecarSocketHealthProbe::Disabled
        );
        assert_eq!(
            sidecar_socket_health_probe(false),
            SidecarSocketHealthProbe::Enabled
        );
    }
}
