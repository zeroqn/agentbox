use anyhow::{anyhow, Result};
use std::process::Stdio;

use super::command::{run_podman, run_podman_capture};

pub fn podman_image_exists(image: &str) -> Result<bool> {
    let args = vec!["image".to_owned(), "exists".to_owned(), image.to_owned()];
    let output = run_podman_capture(args, "failed to check whether default image exists")?;
    Ok(output.status.success())
}

pub fn pull_image(image: &str) -> Result<()> {
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
