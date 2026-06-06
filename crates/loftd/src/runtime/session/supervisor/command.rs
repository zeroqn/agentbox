//! Host-side keep-id helper command construction and process spawning.

use anyhow::{Context, Result};
use std::ffi::OsString;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::logging::INTERNAL_LOG_LEVEL_ENV;
use crate::runtime::launch::config::LaunchConfig;
use crate::runtime::session::profile::{LOFTD_HOST_PROFILE_ENV, LoftdHostProfiler};
use crate::runtime::session::supervisor::identity::KeepIdLauncher;
use crate::runtime::session::supervisor::{ChildStatus, LIBKRUN_ENTER_HELPER_ARG};

pub(crate) fn run_helper_process(
    config: &LaunchConfig,
    config_path: &Path,
    profiler: &mut LoftdHostProfiler,
) -> Result<ChildStatus> {
    let host_profile_enabled = profiler.is_enabled();
    let spec = profiler.measure_result("helper_command_build", || {
        let executable = std::env::current_exe()
            .context("failed to resolve loftd executable for keep-id libkrun helper")?;
        build_helper_command(
            &executable,
            config_path,
            config.log_level,
            host_profile_enabled,
        )
    })?;
    tracing::debug!(program = ?spec.program, args = ?spec.args, log_level = config.log_level.as_str(), "loftd libkrun keep-id helper command constructed");
    let mut command = spec.into_command();
    command
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    let mut child = profiler.measure_result("helper_spawn_process", || {
        command.spawn().with_context(|| {
        format!(
            "failed to start loftd keep-id libkrun helper for '{}'; rootless direct-libkrun launches require util-linux unshare plus newuidmap/newgidmap and usable /etc/subuid + /etc/subgid entries",
            config_path.display()
        )
        })
    })?;
    tracing::debug!(pid = child.id(), "loftd libkrun helper spawned");
    let status = profiler.measure_result("helper_wait_process", || {
        child
            .wait()
            .context("failed to wait for loftd libkrun helper")
    })?;
    tracing::debug!(?status, "loftd libkrun helper exited");
    Ok(match status.code() {
        Some(code) => ChildStatus::exited(code),
        None => ChildStatus::signaled(),
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HelperCommandSpec {
    pub(crate) program: OsString,
    pub(crate) env: Vec<(OsString, OsString)>,
    pub(crate) args: Vec<OsString>,
}

impl HelperCommandSpec {
    fn into_command(self) -> Command {
        let mut command = Command::new(self.program);
        command.args(self.args);
        command.envs(self.env);
        command
    }
}

fn build_helper_command(
    executable: &Path,
    config_path: &Path,
    log_level: crate::logging::LogLevel,
    host_profile_enabled: bool,
) -> Result<HelperCommandSpec> {
    let launcher = KeepIdLauncher::from_current_system()?;
    tracing::debug!(summary = %launcher.diagnostic_summary(), "loftd libkrun helper keep-id namespace resolved");
    Ok(build_helper_command_with_launcher(
        executable,
        config_path,
        log_level,
        host_profile_enabled,
        &launcher,
    ))
}

pub(crate) fn build_helper_command_with_launcher(
    executable: &Path,
    config_path: &Path,
    log_level: crate::logging::LogLevel,
    host_profile_enabled: bool,
    launcher: &KeepIdLauncher,
) -> HelperCommandSpec {
    let mut env = vec![(
        OsString::from(INTERNAL_LOG_LEVEL_ENV),
        OsString::from(log_level.as_str()),
    )];
    if host_profile_enabled {
        env.push((OsString::from(LOFTD_HOST_PROFILE_ENV), OsString::from("1")));
    }
    HelperCommandSpec {
        program: launcher.program(),
        env,
        args: launcher.args(executable, LIBKRUN_ENTER_HELPER_ARG, config_path),
    }
}
