use anyhow::{Context, Result};
use std::path::Path;
use std::process::{Command, ExitCode, Stdio};

use crate::runtime::microvm::ffi::{DirectLibkrunLauncher, DynamicLibkrunApi};
use crate::runtime::microvm::launch::MicrovmLaunchConfig;

pub(crate) const MICROVM_HELPER_ARG: &str = "__agentbox_microvm_enter";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MicrovmChildStatus {
    code: Option<i32>,
}

impl MicrovmChildStatus {
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

pub(crate) trait MicrovmSupervisor {
    fn run(
        &self,
        config: &MicrovmLaunchConfig,
        task_state_dir: &Path,
    ) -> Result<MicrovmChildStatus>;
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct HostMicrovmSupervisor;

impl MicrovmSupervisor for HostMicrovmSupervisor {
    fn run(
        &self,
        config: &MicrovmLaunchConfig,
        task_state_dir: &Path,
    ) -> Result<MicrovmChildStatus> {
        let config_path = task_state_dir.join("launch.conf");
        config.write_to(&config_path)?;
        let status =
            Command::new(std::env::current_exe().context("failed to resolve agentbox executable")?)
                .arg(MICROVM_HELPER_ARG)
                .arg(&config_path)
                .stdin(Stdio::inherit())
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit())
                .status()
                .with_context(|| {
                    format!(
                        "failed to start microvm helper for '{}'",
                        config_path.display()
                    )
                })?;
        Ok(match status.code() {
            Some(code) => MicrovmChildStatus::exited(code),
            None => MicrovmChildStatus::signaled(),
        })
    }
}

pub(crate) fn run_helper(config_path: &Path) -> Result<()> {
    let config = MicrovmLaunchConfig::read_from(config_path)?;
    let api = DynamicLibkrunApi::open_default()?;
    DirectLibkrunLauncher::new(api).start_enter(&config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn child_status_maps_to_parent_exit_code() {
        assert_eq!(MicrovmChildStatus::exited(0).exit_code(), ExitCode::from(0));
        assert_eq!(
            MicrovmChildStatus::exited(42).exit_code(),
            ExitCode::from(42)
        );
        assert_eq!(
            MicrovmChildStatus::exited(300).exit_code(),
            ExitCode::from(1)
        );
        assert_eq!(
            MicrovmChildStatus::signaled().exit_code(),
            ExitCode::from(1)
        );
    }
}
