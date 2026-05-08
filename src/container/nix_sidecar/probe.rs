use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

use crate::podman::command::run_podman_output;

pub fn is_container_running(container_name: &str) -> bool {
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

pub fn path_is_mounted(path: &Path) -> Result<bool> {
    if !path.exists() {
        return Ok(false);
    }

    let target = path
        .canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .to_string();

    let mountinfo = fs::read_to_string("/proc/self/mountinfo")
        .context("failed to read /proc/self/mountinfo for mount health check")?;

    for line in mountinfo.lines() {
        let mut fields = line.split_whitespace();
        let _mount_id = fields.next();
        let _parent_id = fields.next();
        let _major_minor = fields.next();
        let _root = fields.next();
        let mount_point = fields.next();

        if mount_point == Some(target.as_str()) {
            return Ok(true);
        }
    }

    Ok(false)
}
