use anyhow::{anyhow, Context, Result};
use std::process::{Command, Stdio};

pub fn run_podman(
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

pub fn run_podman_output(args: Vec<String>, context: &str) -> Result<String> {
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

pub fn run_podman_capture(args: Vec<String>, context: &str) -> Result<std::process::Output> {
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
