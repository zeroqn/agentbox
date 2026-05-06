use anyhow::{anyhow, Context, Result};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};

static PODMAN_DEBUG: AtomicBool = AtomicBool::new(false);

pub fn set_podman_debug(enabled: bool) {
    PODMAN_DEBUG.store(enabled, Ordering::Relaxed);
}

pub(crate) fn podman_debug_args() -> Vec<String> {
    podman_debug_args_for(PODMAN_DEBUG.load(Ordering::Relaxed))
}

pub(crate) fn podman_debug_args_for(debug: bool) -> Vec<String> {
    if debug {
        vec!["--log-level=debug".to_owned()]
    } else {
        Vec::new()
    }
}

pub(crate) fn podman_args_for_debug(mut args: Vec<String>, debug: bool) -> Vec<String> {
    let mut debug_args = podman_debug_args_for(debug);
    debug_args.append(&mut args);
    debug_args
}

pub(crate) fn podman_command() -> Command {
    let mut command = Command::new("podman");
    command.args(podman_args_for_debug(
        Vec::new(),
        PODMAN_DEBUG.load(Ordering::Relaxed),
    ));
    command
}

pub fn run_podman(
    args: Vec<String>,
    stdin: Stdio,
    stdout: Stdio,
    stderr: Stdio,
    context: &str,
) -> Result<std::process::ExitStatus> {
    podman_command()
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

pub fn run_podman_output(args: Vec<String>, context: &str) -> Result<String> {
    let output = podman_command()
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

pub fn run_podman_capture(args: Vec<String>, context: &str) -> Result<std::process::Output> {
    podman_command()
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
