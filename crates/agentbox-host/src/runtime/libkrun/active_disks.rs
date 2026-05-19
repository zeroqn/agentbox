use anyhow::Result;
use std::path::Path;

use crate::podman::command::run_podman_output;

pub trait PodmanOutputRunner {
    fn output(&self, args: Vec<String>, context: &str) -> Result<String>;
}

#[derive(Debug, Clone, Copy)]
pub struct HostPodmanOutputRunner;

impl PodmanOutputRunner for HostPodmanOutputRunner {
    fn output(&self, args: Vec<String>, context: &str) -> Result<String> {
        run_podman_output(args, context)
    }
}

pub fn ensure_no_live_raw_disk_users(
    path: &Path,
    operation: &str,
    runner: &impl PodmanOutputRunner,
) -> Result<()> {
    let users = live_raw_disk_users(path, operation, runner)?;
    if !users.is_empty() {
        anyhow::bail!(
            "refusing to {operation} libkrun raw image '{}' while running Podman container(s) use it: {}",
            path.display(),
            users.join(", ")
        );
    }
    Ok(())
}

fn live_raw_disk_users(
    path: &Path,
    operation: &str,
    runner: &impl PodmanOutputRunner,
) -> Result<Vec<String>> {
    let ps = runner.output(
        vec!["ps".to_owned(), "--format".to_owned(), "{{.ID}}".to_owned()],
        &format!("failed to list running Podman containers before libkrun {operation}"),
    )?;
    let target = path.to_string_lossy();
    let mut users = Vec::new();
    for id in ps.lines().map(str::trim).filter(|line| !line.is_empty()) {
        let annotations = runner.output(
            vec![
                "inspect".to_owned(),
                "--format".to_owned(),
                "{{range $key, $value := .Config.Annotations}}{{printf \"%s=%s\\n\" $key $value}}{{end}}"
                    .to_owned(),
                id.to_owned(),
            ],
            &format!("failed to inspect running Podman container annotations before libkrun {operation}"),
        )?;
        if annotations_use_raw_disk(&annotations, &target) {
            users.push(id.to_owned());
        }
    }
    Ok(users)
}

fn annotations_use_raw_disk(annotations: &str, target_path: &str) -> bool {
    annotations.lines().any(|line| {
        let Some((key, value)) = line.split_once('=') else {
            return false;
        };
        key.starts_with("krun.disk.") && key.ends_with(".path") && value == target_path
    })
}

#[cfg(test)]
mod tests {
    use crate::runtime::libkrun::active_disks::{
        PodmanOutputRunner, ensure_no_live_raw_disk_users, live_raw_disk_users,
    };
    use anyhow::{Result, anyhow};
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::path::Path;

    #[test]
    fn live_probe_blocks_matching_krun_disk_path_and_ignores_non_matching() {
        let target = Path::new("/tmp/state/libkrun-nix.raw");
        let runner = FakePodmanOutputRunner::new([
            Ok("abc\ndef\n".to_owned()),
            Ok("krun.disk.0.path=/tmp/state/libkrun-nix.raw\n".to_owned()),
            Ok("krun.disk.1.path=/other.raw\n".to_owned()),
        ]);

        let users = live_raw_disk_users(target, "reset", &runner).unwrap();

        assert_eq!(users, ["abc"]);
    }

    #[test]
    fn live_probe_fails_closed_on_podman_errors() {
        let target = Path::new("/tmp/state/libkrun-nix.raw");
        let runner = FakePodmanOutputRunner::new([Err("podman unavailable".to_owned())]);

        let err = ensure_no_live_raw_disk_users(target, "reset", &runner).unwrap_err();

        assert!(err.to_string().contains("podman unavailable"));
    }

    struct FakePodmanOutputRunner {
        outputs: RefCell<VecDeque<Result<String, String>>>,
    }

    impl FakePodmanOutputRunner {
        fn new<const N: usize>(outputs: [Result<String, String>; N]) -> Self {
            Self {
                outputs: RefCell::new(outputs.into()),
            }
        }
    }

    impl PodmanOutputRunner for FakePodmanOutputRunner {
        fn output(&self, _args: Vec<String>, _context: &str) -> Result<String> {
            match self.outputs.borrow_mut().pop_front() {
                Some(Ok(output)) => Ok(output),
                Some(Err(message)) => Err(anyhow!(message)),
                None => Err(anyhow!("unexpected podman call")),
            }
        }
    }
}
