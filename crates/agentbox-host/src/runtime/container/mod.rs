mod nix_sidecar;
mod task;

use anyhow::{Context, Result};
use std::env;
use std::process::{ExitCode, Stdio};

use crate::cli::{resolve_image, CommonOptions, ContainerMode, ContainerOptions};
use crate::podman::command::run_podman;
use crate::runtime::components::volumes::prepare_task_volumes;
use crate::runtime::container::nix_sidecar::{
    cleanup_idle_sidecar, prepare_sidecar_nix_runtime, SidecarDaemonRuntimeSpec,
    SidecarSocketHealthProbe,
};
use crate::state::resolve_state_layout;
use crate::{derive_task_container_name, derive_task_hostname};

use task::{build_container_task_podman_args, ContainerTaskPodmanSpec};

pub(crate) fn run(common: CommonOptions, options: ContainerOptions) -> Result<ExitCode> {
    let mode = options.mode();
    let cwd = env::current_dir()
        .context("failed to resolve current directory")?
        .canonicalize()
        .context("failed to canonicalize current directory")?;
    let image = resolve_image(common.image.as_deref(), common.pull_latest)?;
    let state_layout = resolve_state_layout(&cwd)?;

    if !should_launch_task_container(mode) {
        let sidecar = prepare_sidecar_nix_runtime(
            &cwd,
            state_layout.root_dir(),
            &image,
            SidecarDaemonRuntimeSpec {
                socket_health_probe: sidecar_socket_health_probe(mode),
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
    let task_volumes = prepare_task_volumes(&cwd, &state_layout)?;

    let nix_runtime = prepare_sidecar_nix_runtime(
        &cwd,
        state_layout.root_dir(),
        &image,
        SidecarDaemonRuntimeSpec {
            socket_health_probe: sidecar_socket_health_probe(mode),
        },
    )?;

    let status = run_podman(
        build_container_task_podman_args(ContainerTaskPodmanSpec {
            image: &image,
            container_name: &task_container_name,
            hostname: &task_hostname,
            task_volumes: &task_volumes,
            nix_runtime: &nix_runtime,
            guest_profile: common.profile,
            guest_debug: common.debug,
        })?,
        Stdio::inherit(),
        Stdio::inherit(),
        Stdio::inherit(),
        "failed to start podman",
    )?;

    if should_cleanup_idle_sidecar_after_run(mode) {
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

fn should_launch_task_container(mode: ContainerMode) -> bool {
    mode == ContainerMode::Task
}

fn should_cleanup_idle_sidecar_after_run(mode: ContainerMode) -> bool {
    mode == ContainerMode::Task
}

fn sidecar_socket_health_probe(mode: ContainerMode) -> SidecarSocketHealthProbe {
    match mode {
        ContainerMode::Task => SidecarSocketHealthProbe::Enabled,
        ContainerMode::Sidecar => SidecarSocketHealthProbe::Disabled,
    }
}

#[cfg(test)]
mod tests {
    use crate::cli::ContainerMode;
    use crate::runtime::container::nix_sidecar::SidecarSocketHealthProbe;
    use crate::runtime::container::{
        should_cleanup_idle_sidecar_after_run, should_launch_task_container,
        sidecar_socket_health_probe,
    };

    #[test]
    fn sidecar_branch_skips_task_launch_and_idle_cleanup() {
        assert!(!should_launch_task_container(ContainerMode::Sidecar));
        assert!(!should_cleanup_idle_sidecar_after_run(
            ContainerMode::Sidecar
        ));
        assert!(should_launch_task_container(ContainerMode::Task));
        assert!(should_cleanup_idle_sidecar_after_run(ContainerMode::Task));
    }

    #[test]
    fn sidecar_branch_disables_socket_health_probe() {
        assert_eq!(
            sidecar_socket_health_probe(ContainerMode::Sidecar),
            SidecarSocketHealthProbe::Disabled
        );
        assert_eq!(
            sidecar_socket_health_probe(ContainerMode::Task),
            SidecarSocketHealthProbe::Enabled
        );
    }
}
