use anyhow::{Context, Result, anyhow};
use std::process::Stdio;

pub(in crate::runtime::container::nix_sidecar) fn ensure_command_available(
    command: &str,
    guidance: &str,
) -> Result<()> {
    let status = std::process::Command::new(command)
        .arg("--help")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();

    match status {
        Ok(_) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Err(anyhow!(
            "{} is not installed or not available on PATH; {}",
            command,
            guidance
        )),
        Err(err) => Err(err).with_context(|| format!("failed to execute '{}'", command)),
    }
}
