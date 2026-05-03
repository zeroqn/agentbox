use anyhow::{Context, Result};

use crate::mounts::format::{format_mount_arg, format_mount_arg_with_options};
use crate::{
    NixRuntime, TaskContainerMode, CONTAINER_NIX_DIR, CONTAINER_SCCACHE_DIR, CONTAINER_TMP_TMPFS,
    CONTAINER_WORKDIR, HOST_GID_ENV_PREFIX, HOST_UID_ENV_PREFIX, INTERACTIVE_SHELL,
    KVM_NIX_PROXY_GUEST_NIX_REMOTE, KVM_NIX_PROXY_HOST_ENV, KVM_NIX_PROXY_PORT_ENV,
    NIX_REMOTE_SOCKET, TASK_CONTAINER_ROLE_LABEL, TASK_CONTAINER_ROLE_VALUE,
    TASK_CONTAINER_SIDECAR_LABEL, TASK_KVM_DROP_TO_DEV_ENV,
};

pub struct TaskPodmanSpec<'a> {
    pub image: &'a str,
    pub hostname: &'a str,
    pub workspace_mount: &'a str,
    pub codex_mount: &'a str,
    pub cargo_mount: &'a str,
    pub sccache_mount: &'a str,
    pub nix_runtime: &'a NixRuntime,
    pub task_mode: TaskContainerMode,
    pub proxy_port: Option<u16>,
}

pub fn build_podman_args(spec: TaskPodmanSpec<'_>) -> Result<Vec<String>> {
    let mut args = vec![
        "run".to_owned(),
        "--rm".to_owned(),
        "-it".to_owned(),
        "--userns".to_owned(),
        "keep-id".to_owned(),
        "--workdir".to_owned(),
        CONTAINER_WORKDIR.to_owned(),
        "--hostname".to_owned(),
        spec.hostname.to_owned(),
        "--volume".to_owned(),
        spec.workspace_mount.to_owned(),
        "--volume".to_owned(),
        spec.codex_mount.to_owned(),
        "--volume".to_owned(),
        spec.cargo_mount.to_owned(),
        "--volume".to_owned(),
        spec.sccache_mount.to_owned(),
        "--env".to_owned(),
        format!("SCCACHE_DIR={CONTAINER_SCCACHE_DIR}"),
        "--tmpfs".to_owned(),
        CONTAINER_TMP_TMPFS.to_owned(),
    ];

    if spec.task_mode == TaskContainerMode::KvmKrunExperimental {
        args.push("--env".to_owned());
        args.push(TASK_KVM_DROP_TO_DEV_ENV.to_owned());
        args.push("--env".to_owned());
        args.push(format!(
            "{HOST_UID_ENV_PREFIX}{}",
            run_host_id_command("-u")?
        ));
        args.push("--env".to_owned());
        args.push(format!(
            "{HOST_GID_ENV_PREFIX}{}",
            run_host_id_command("-g")?
        ));
        args.push("--runtime".to_owned());
        args.push("crun".to_owned());
        args.push("--annotation".to_owned());
        args.push("run.oci.handler=krun".to_owned());
    }

    match spec.nix_runtime {
        NixRuntime::Seeded(persistent_nix_root) => {
            for (source, destination) in persistent_nix_root.mounts() {
                args.push("--volume".to_owned());
                args.push(format_mount_arg(source, destination)?);
            }
        }
        NixRuntime::Sidecar(sidecar) => {
            args.push("--volume".to_owned());
            args.push(format_mount_arg_with_options(
                &sidecar.merged_dir,
                CONTAINER_NIX_DIR,
                Some("ro"),
            )?);

            let nix_remote = if spec.task_mode == TaskContainerMode::KvmKrunExperimental {
                KVM_NIX_PROXY_GUEST_NIX_REMOTE
            } else {
                NIX_REMOTE_SOCKET
            };
            args.push("--env".to_owned());
            args.push(format!("NIX_REMOTE={nix_remote}"));

            if spec.task_mode == TaskContainerMode::KvmKrunExperimental {
                if let Some(port) = spec.proxy_port {
                    let host_ip = resolve_host_ip()?;
                    args.push("--env".to_owned());
                    args.push(format!("{KVM_NIX_PROXY_HOST_ENV}={host_ip}"));
                    args.push("--env".to_owned());
                    args.push(format!("{KVM_NIX_PROXY_PORT_ENV}={port}"));
                }
            }

            args.push("--label".to_owned());
            args.push(format!(
                "{TASK_CONTAINER_ROLE_LABEL}={TASK_CONTAINER_ROLE_VALUE}"
            ));
            args.push("--label".to_owned());
            args.push(format!(
                "{TASK_CONTAINER_SIDECAR_LABEL}={}",
                sidecar.sidecar_name
            ));
        }
    }

    args.push(spec.image.to_owned());
    args.push(INTERACTIVE_SHELL.to_owned());
    args.push("-l".to_owned());
    Ok(args)
}

fn run_host_id_command(flag: &str) -> Result<String> {
    let output = std::process::Command::new("id")
        .arg(flag)
        .output()
        .context("failed to run id")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("id {} failed: {}", flag, stderr.trim());
    }
    String::from_utf8(output.stdout)
        .context("id output not valid UTF-8")
        .map(|s| s.trim().to_owned())
}

#[cfg(not(test))]
fn resolve_host_ip() -> Result<String> {
    let output = std::process::Command::new("hostname")
        .arg("-I")
        .output()
        .context("failed to run hostname -I to resolve host IP")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("hostname -I failed: {}", stderr.trim());
    }
    let stdout = String::from_utf8(output.stdout)
        .context("hostname -I output not valid UTF-8")?;
    let ip = stdout
        .split_whitespace()
        .next()
        .ok_or_else(|| anyhow::anyhow!(
            "hostname -I returned no IP addresses; is the host network configured?"
        ))?;
    Ok(ip.to_owned())
}

#[cfg(test)]
fn resolve_host_ip() -> Result<String> {
    Ok("127.0.0.1".to_owned())
}
