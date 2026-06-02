use anyhow::{Context, Result, anyhow};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

use crate::runtime::ffi::{DirectLibkrunLauncher, DynamicLibkrunApi};
use crate::runtime::launch_config::LaunchConfig;

pub(crate) const LIBKRUN_ENTER_HELPER_ARG: &str = "libkrun-enter";
const BUILDAH_PROGRAM: &str = "buildah";
const BUILDAH_UNSHARE_ARG: &str = "unshare";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ChildStatus {
    code: Option<i32>,
}

impl ChildStatus {
    pub(crate) fn exited(code: i32) -> Self {
        Self { code: Some(code) }
    }

    pub(crate) fn signaled() -> Self {
        Self { code: None }
    }

    pub(crate) fn exit_code(self) -> ExitCode {
        ExitCode::from(
            self.code
                .and_then(|code| u8::try_from(code).ok())
                .unwrap_or(1),
        )
    }
}

pub(crate) trait Supervisor {
    fn run(&self, config: &LaunchConfig, task_state_dir: &Path) -> Result<ChildStatus>;
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct HostSupervisor;

impl Supervisor for HostSupervisor {
    fn run(&self, config: &LaunchConfig, task_state_dir: &Path) -> Result<ChildStatus> {
        let config_path = task_state_dir.join("launch.conf");
        config.write_to(&config_path)?;
        run_helper_process(&config_path)
    }
}

fn run_helper_process(config_path: &Path) -> Result<ChildStatus> {
    let executable = std::env::current_exe()
        .context("failed to resolve loftd executable for buildah unshare libkrun helper")?;
    let mut command = build_helper_command(&executable, config_path).into_command();
    let status = command
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .with_context(|| {
            format!(
                "failed to start buildah unshare loftd libkrun helper for '{}'; btrfs-snapshot direct-libkrun launches require buildah",
                config_path.display()
            )
        })?;
    Ok(match status.code() {
        Some(code) => ChildStatus::exited(code),
        None => ChildStatus::signaled(),
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HelperCommandSpec {
    program: OsString,
    args: Vec<OsString>,
}

impl HelperCommandSpec {
    fn into_command(self) -> Command {
        let mut command = Command::new(self.program);
        command.args(self.args);
        command
    }
}

fn build_helper_command(executable: &Path, config_path: &Path) -> HelperCommandSpec {
    HelperCommandSpec {
        program: OsString::from(BUILDAH_PROGRAM),
        args: vec![
            OsString::from(BUILDAH_UNSHARE_ARG),
            executable.as_os_str().to_os_string(),
            OsString::from("internal"),
            OsString::from(LIBKRUN_ENTER_HELPER_ARG),
            config_path.as_os_str().to_os_string(),
        ],
    }
}

pub(crate) fn run_internal(args: Vec<OsString>) -> Result<()> {
    let [subcommand, config_path]: [OsString; 2] = args.try_into().map_err(|args: Vec<_>| {
        anyhow!(
            "expected internal {LIBKRUN_ENTER_HELPER_ARG} <launch.conf>, got {} argument(s)",
            args.len()
        )
    })?;
    if subcommand.to_str() != Some(LIBKRUN_ENTER_HELPER_ARG) {
        anyhow::bail!(
            "unknown loftd internal command '{}'; expected {LIBKRUN_ENTER_HELPER_ARG}",
            subcommand.to_string_lossy()
        );
    }
    run_helper(PathBuf::from(config_path).as_path())
}

fn run_helper(config_path: &Path) -> Result<()> {
    let config = LaunchConfig::read_from(config_path)?;
    let api = DynamicLibkrunApi::open_default()?;
    DirectLibkrunLauncher::new(api).start_enter(&config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn child_status_maps_to_parent_exit_code() {
        assert_eq!(ChildStatus::exited(0).exit_code(), ExitCode::from(0));
        assert_eq!(ChildStatus::exited(42).exit_code(), ExitCode::from(42));
        assert_eq!(ChildStatus::exited(300).exit_code(), ExitCode::from(1));
        assert_eq!(ChildStatus::signaled().exit_code(), ExitCode::from(1));
    }

    #[test]
    fn internal_rejects_wrong_argument_shape() {
        let err = run_internal(vec!["libkrun-enter".into()]).expect_err("missing config path");
        assert!(format!("{err:#}").contains("expected internal"));

        let err = run_internal(vec!["btrfs-rootfs".into(), "/tmp/x".into()])
            .expect_err("wrong subcommand");
        assert!(format!("{err:#}").contains("unknown loftd internal command"));
    }

    #[test]
    fn libkrun_helper_launches_through_buildah_unshare() {
        let spec = build_helper_command(
            Path::new("/nix/store/hash-loftd/bin/loftd"),
            Path::new("/tmp/loftd-task/launch.conf"),
        );

        assert_eq!(spec.program, OsString::from("buildah"));
        assert_eq!(
            spec.args,
            vec![
                OsString::from("unshare"),
                OsString::from("/nix/store/hash-loftd/bin/loftd"),
                OsString::from("internal"),
                OsString::from("libkrun-enter"),
                OsString::from("/tmp/loftd-task/launch.conf"),
            ]
        );
    }
}
