use anyhow::{Context, Result};
use std::env;
use std::fs;
use std::path::Path;
use std::process::ExitCode;

use crate::cli::LibkrunResetNixOptions;
use crate::runtime::libkrun::active_disks::{
    HostPodmanOutputRunner, PodmanOutputRunner, ensure_no_live_raw_disk_users,
};
use crate::runtime::libkrun::components::disk::nix::raw_image::RAW_NIX_DISK_SPEC;
use crate::runtime::libkrun::components::disk::raw_btrfs::{
    HostRawImageCommandRunner, RawBtrfsDisk, RawImageCommandRunner, prepare_path_with_runner,
};
use crate::state::resolve_state_layout;

pub fn run(reset_options: LibkrunResetNixOptions) -> Result<ExitCode> {
    let cwd = env::current_dir()
        .context("failed to resolve current directory")?
        .canonicalize()
        .context("failed to canonicalize current directory")?;
    let state_layout = resolve_state_layout(&cwd)?;

    reset_nix_disk_with_runner(
        state_layout.root_dir(),
        reset_options.force,
        &HostPodmanOutputRunner,
        &HostRawImageCommandRunner,
    )?;

    Ok(ExitCode::SUCCESS)
}

fn reset_nix_disk_with_runner(
    state_root: &Path,
    force: bool,
    podman_runner: &impl PodmanOutputRunner,
    raw_runner: &impl RawImageCommandRunner,
) -> Result<RawBtrfsDisk> {
    if !force {
        anyhow::bail!(
            "refusing to reset libkrun /nix raw image without --force; this deletes and recreates {} with no backup",
            RAW_NIX_DISK_SPEC.file_name
        );
    }

    let path = state_root.join(RAW_NIX_DISK_SPEC.file_name);
    ensure_no_live_raw_disk_users(&path, "reset", podman_runner)?;
    remove_existing_managed_file(&path)?;
    prepare_path_with_runner(&path, &RAW_NIX_DISK_SPEC, raw_runner)
}

fn remove_existing_managed_file(path: &Path) -> Result<()> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => {
            return Err(err).with_context(|| {
                format!(
                    "failed to inspect existing libkrun /nix raw image '{}'",
                    path.display()
                )
            });
        }
    };

    if !metadata.file_type().is_file() {
        anyhow::bail!(
            "existing libkrun /nix raw image path '{}' is not a regular file; refusing to reset it",
            path.display()
        );
    }

    fs::remove_file(path).with_context(|| {
        format!(
            "failed to delete existing libkrun /nix raw image '{}'",
            path.display()
        )
    })
}

#[cfg(test)]
mod tests {
    use crate::runtime::libkrun::active_disks::PodmanOutputRunner;
    use crate::runtime::libkrun::components::disk::nix::raw_image::RAW_NIX_DISK_SPEC;
    use crate::runtime::libkrun::components::disk::raw_btrfs::{
        RawBtrfsDiskStatus, test_support::FakeRunner,
    };
    use crate::runtime::libkrun::reset::reset_nix_disk_with_runner;
    use anyhow::{Result, anyhow};
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn reset_nix_requires_force_before_mutating_or_probing() {
        let temp = tempdir().unwrap();
        let path = temp.path().join(RAW_NIX_DISK_SPEC.file_name);
        fs::write(&path, b"keep-me").unwrap();
        let podman_runner = FakePodmanOutputRunner::new([]);
        let raw_runner = FakeRunner::default();

        let err = reset_nix_disk_with_runner(temp.path(), false, &podman_runner, &raw_runner)
            .unwrap_err();

        assert!(err.to_string().contains("without --force"));
        assert_eq!(fs::read(&path).unwrap(), b"keep-me");
        assert_eq!(podman_runner.call_count(), 0);
        assert_eq!(raw_runner.mkfs_call_count(), 0);
    }

    #[test]
    fn reset_nix_replaces_existing_file_with_default_btrfs_image() {
        let temp = tempdir().unwrap();
        let path = temp.path().join(RAW_NIX_DISK_SPEC.file_name);
        fs::write(&path, b"old nix store").unwrap();
        let podman_runner = FakePodmanOutputRunner::new([Ok(String::new())]);
        let raw_runner = FakeRunner::default();

        let disk = reset_nix_disk_with_runner(temp.path(), true, &podman_runner, &raw_runner)
            .expect("forced inactive reset should recreate the nix raw image");

        assert_eq!(disk.path, path);
        assert_eq!(disk.status, RawBtrfsDiskStatus::Created);
        assert_eq!(disk.size_bytes, RAW_NIX_DISK_SPEC.size_bytes);
        assert_eq!(
            disk.path.metadata().unwrap().len(),
            RAW_NIX_DISK_SPEC.size_bytes
        );
        assert_eq!(raw_runner.mkfs_call_count(), 1);
        assert_eq!(raw_runner.mkfs_calls.borrow()[0].1, RAW_NIX_DISK_SPEC.label);
    }

    #[test]
    fn reset_nix_creates_missing_image() {
        let temp = tempdir().unwrap();
        let path = temp.path().join(RAW_NIX_DISK_SPEC.file_name);
        let podman_runner = FakePodmanOutputRunner::new([Ok(String::new())]);
        let raw_runner = FakeRunner::default();

        let disk = reset_nix_disk_with_runner(temp.path(), true, &podman_runner, &raw_runner)
            .expect("forced inactive reset should create the missing nix raw image");

        assert_eq!(disk.path, path);
        assert_eq!(disk.status, RawBtrfsDiskStatus::Created);
        assert_eq!(raw_runner.mkfs_call_count(), 1);
    }

    #[test]
    fn reset_nix_refuses_active_users_before_delete() {
        let temp = tempdir().unwrap();
        let path = temp.path().join(RAW_NIX_DISK_SPEC.file_name);
        fs::write(&path, b"keep-me").unwrap();
        let podman_runner = FakePodmanOutputRunner::new([
            Ok("abc\n".to_owned()),
            Ok(format!("krun.disk.0.path={}\n", path.display())),
        ]);
        let raw_runner = FakeRunner::default();

        let err =
            reset_nix_disk_with_runner(temp.path(), true, &podman_runner, &raw_runner).unwrap_err();

        assert!(err.to_string().contains("refusing to reset"));
        assert!(err.to_string().contains("abc"));
        assert_eq!(fs::read(&path).unwrap(), b"keep-me");
        assert_eq!(raw_runner.mkfs_call_count(), 0);
    }

    #[test]
    fn reset_nix_refuses_non_file_existing_path() {
        let temp = tempdir().unwrap();
        fs::create_dir(temp.path().join(RAW_NIX_DISK_SPEC.file_name)).unwrap();
        let podman_runner = FakePodmanOutputRunner::new([Ok(String::new())]);
        let raw_runner = FakeRunner::default();

        let err =
            reset_nix_disk_with_runner(temp.path(), true, &podman_runner, &raw_runner).unwrap_err();

        assert!(err.to_string().contains("not a regular file"));
        assert_eq!(raw_runner.mkfs_call_count(), 0);
    }

    struct FakePodmanOutputRunner {
        outputs: RefCell<VecDeque<Result<String, String>>>,
        calls: RefCell<usize>,
    }

    impl FakePodmanOutputRunner {
        fn new<const N: usize>(outputs: [Result<String, String>; N]) -> Self {
            Self {
                outputs: RefCell::new(outputs.into()),
                calls: RefCell::new(0),
            }
        }

        fn call_count(&self) -> usize {
            *self.calls.borrow()
        }
    }

    impl PodmanOutputRunner for FakePodmanOutputRunner {
        fn output(&self, _args: Vec<String>, _context: &str) -> Result<String> {
            *self.calls.borrow_mut() += 1;
            match self.outputs.borrow_mut().pop_front() {
                Some(Ok(output)) => Ok(output),
                Some(Err(message)) => Err(anyhow!(message)),
                None => Err(anyhow!("unexpected podman call")),
            }
        }
    }
}
