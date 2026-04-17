use anyhow::{anyhow, Context, Result};
use std::process::{Command, Stdio};

use crate::{
    format_mount_arg, format_mount_arg_with_options, NixRuntime, CONTAINER_NIX_DIR,
    CONTAINER_TMP_TMPFS, CONTAINER_WORKDIR, INTERACTIVE_SHELL, NIX_REMOTE_SOCKET,
    TASK_CONTAINER_ROLE_LABEL, TASK_CONTAINER_ROLE_VALUE, TASK_CONTAINER_SIDECAR_LABEL,
};

pub(crate) fn podman_image_exists(image: &str) -> Result<bool> {
    let args = vec!["image".to_owned(), "exists".to_owned(), image.to_owned()];
    let output = run_podman_capture(args, "failed to check whether default image exists")?;
    Ok(output.status.success())
}

pub(crate) fn pull_image(image: &str) -> Result<()> {
    let args = vec!["pull".to_owned(), image.to_owned()];
    let status = run_podman(
        args,
        Stdio::null(),
        Stdio::inherit(),
        Stdio::inherit(),
        "failed to pull container image",
    )?;
    if !status.success() {
        return Err(anyhow!("podman pull '{}' failed", image));
    }

    Ok(())
}

pub(crate) fn build_podman_args(
    image: &str,
    workspace_mount: &str,
    codex_mount: &str,
    cargo_mount: &str,
    nix_runtime: &NixRuntime,
) -> Result<Vec<String>> {
    let mut args = vec![
        "run".to_owned(),
        "--rm".to_owned(),
        "-it".to_owned(),
        "--userns".to_owned(),
        "keep-id".to_owned(),
        "--workdir".to_owned(),
        CONTAINER_WORKDIR.to_owned(),
        "--volume".to_owned(),
        workspace_mount.to_owned(),
        "--volume".to_owned(),
        codex_mount.to_owned(),
        "--volume".to_owned(),
        cargo_mount.to_owned(),
        "--tmpfs".to_owned(),
        CONTAINER_TMP_TMPFS.to_owned(),
    ];

    match nix_runtime {
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
            args.push("--env".to_owned());
            args.push(format!("NIX_REMOTE={NIX_REMOTE_SOCKET}"));
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

    args.push(image.to_owned());
    args.push(INTERACTIVE_SHELL.to_owned());
    args.push("-l".to_owned());
    Ok(args)
}

pub(crate) fn run_podman(
    args: Vec<String>,
    stdin: Stdio,
    stdout: Stdio,
    stderr: Stdio,
    context: &str,
) -> Result<std::process::ExitStatus> {
    Command::new("podman")
        .args(args)
        .stdin(stdin)
        .stdout(stdout)
        .stderr(stderr)
        .status()
        .map_err(|err| match err.kind() {
            std::io::ErrorKind::NotFound => {
                anyhow!("podman is not installed or not available on PATH")
            }
            _ => err.into(),
        })
        .with_context(|| context.to_owned())
}

pub(crate) fn run_podman_output(args: Vec<String>, context: &str) -> Result<String> {
    let output = Command::new("podman")
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|err| match err.kind() {
            std::io::ErrorKind::NotFound => {
                anyhow!("podman is not installed or not available on PATH")
            }
            _ => err.into(),
        })
        .with_context(|| context.to_owned())?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        if stderr.is_empty() {
            return Err(anyhow!("{}", context));
        }
        return Err(anyhow!("{}: {}", context, stderr));
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

pub(crate) fn run_podman_capture(args: Vec<String>, context: &str) -> Result<std::process::Output> {
    Command::new("podman")
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|err| match err.kind() {
            std::io::ErrorKind::NotFound => {
                anyhow!("podman is not installed or not available on PATH")
            }
            _ => err.into(),
        })
        .with_context(|| context.to_owned())
}

pub(crate) fn build_podman_unshare_args(mut args: Vec<String>) -> Vec<String> {
    let mut wrapped = Vec::with_capacity(args.len() + 2);
    wrapped.push("unshare".to_owned());
    wrapped.push("podman".to_owned());
    wrapped.append(&mut args);
    wrapped
}
