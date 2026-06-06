//! Internal helper entrypoint and launch-config dispatch.

use anyhow::{Result, anyhow, bail};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::logging::{self, LogSettings};
use crate::runtime::launch::config::{LaunchConfig, NetworkMode};
use crate::runtime::session::supervisor::LIBKRUN_ENTER_HELPER_ARG;
use crate::runtime::session::supervisor::identity;
use crate::runtime::session::supervisor::vm_child::{self, VmWorkerGuard};
use crate::runtime::vm::network::{self, NetworkManagerSession};

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
    if logging::helper_pre_config_debug_enabled() {
        eprintln!(
            "loftd internal: libkrun-network-enter starting config={}",
            config_path.display()
        );
    }
    let config = LaunchConfig::read_from(config_path)?;
    logging::init_tracing(&LogSettings::for_internal_helper(config.log_level))?;
    identity::configure_helper_filesystem_identity(&config)?;
    let task_state_dir = task_state_dir_from_config_path(config_path)?;
    tracing::debug!(
        mode = config.network_mode.as_config_value(),
        "loftd internal: network manager starting"
    );
    let mut network_session = NetworkManagerSession::start(task_state_dir)?;
    let (passt_read, passt_write) = if config.network_mode == NetworkMode::Passt {
        let (read_fd, write_fd) = network::passt_pid_pipe()?;
        (Some(read_fd), Some(write_fd))
    } else {
        (None, None)
    };
    let mut worker = VmWorkerGuard::new(vm_child::fork_vm_worker(
        config_path,
        network_session.holder_pid(),
        passt_write,
    )?);
    let passt_pid = if config.network_mode == NetworkMode::Passt {
        passt_read
            .map(network::read_passt_pid)
            .transpose()?
            .flatten()
    } else {
        None
    };
    network_session.set_passt_pid(passt_pid);
    let status = worker.wait()?;
    if let Some(code) = network::status_exit_code(status) {
        if code == 0 {
            return Ok(());
        }
        bail!("loftd VM worker exited with status {code}");
    }
    bail!("loftd VM worker exited due to signal")
}

pub(crate) fn task_state_dir_from_config_path(config_path: &Path) -> Result<&Path> {
    config_path.parent().ok_or_else(|| {
        anyhow!(
            "loftd launch config '{}' must live inside a task state directory",
            config_path.display()
        )
    })
}
