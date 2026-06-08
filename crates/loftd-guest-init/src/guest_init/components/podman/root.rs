use anyhow::{Context, Result, bail};
use std::fs::OpenOptions;
use std::io::Write;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::guest_init::command;
use crate::guest_init::components::env::{
    ContainerStoreBackend, LoftdEnv, PODMAN_LOG_PATH, PODMAN_STATUS_PATH, RUN_DIR,
};
use crate::guest_init::components::home::identity::DevIdentity;
use crate::guest_init::components::podman::config::PodmanToolPaths;
use crate::guest_init::components::podman::status::{self, PodmanPrepState, PodmanPrepStatus};
use crate::guest_init::{fs, process};

const WAIT_FOR_STATUS_ENV: &str = "LOFTD_PODMAN_PREP_WAIT_FOR_STATUS";

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::guest_init) enum PodmanPrepOperation {
    WriteRunningStatus,
    EnableUserNamespaces,
    PrepareTun,
    PrepareKvm,
    MaterializeSubids,
    InstallIdmapHelpers,
    MountContainerStorage,
    WriteConfig,
    WriteReadyStatus,
}

#[cfg(test)]
pub(in crate::guest_init) fn planned_operations() -> Vec<PodmanPrepOperation> {
    vec![
        PodmanPrepOperation::WriteRunningStatus,
        PodmanPrepOperation::EnableUserNamespaces,
        PodmanPrepOperation::PrepareTun,
        PodmanPrepOperation::PrepareKvm,
        PodmanPrepOperation::MaterializeSubids,
        PodmanPrepOperation::InstallIdmapHelpers,
        PodmanPrepOperation::MountContainerStorage,
        PodmanPrepOperation::WriteConfig,
        PodmanPrepOperation::WriteReadyStatus,
    ]
}

pub(in crate::guest_init) fn start_background_prep(
    identity: &DevIdentity,
    env_contract: &LoftdEnv,
) -> Result<()> {
    if !env_contract.containers_storage {
        return Ok(());
    }
    if !process::is_root() {
        bail!("rootless Podman root prep must start as root");
    }
    fs::create_dir_all(Path::new(RUN_DIR))?;
    let status_path = PathBuf::from(PODMAN_STATUS_PATH);
    let log_path = PathBuf::from(PODMAN_LOG_PATH);
    let current_exe = std::env::current_exe().context("failed to resolve guest-init executable")?;
    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .with_context(|| format!("failed to open {}", log_path.display()))?;
    fs::chown(&log_path, identity.uid, identity.gid)?;
    fs::chmod(&log_path, 0o644)?;
    let log_err = log.try_clone()?;
    let child = unsafe {
        Command::new(current_exe)
            .args(["internal", "podman", "prep"])
            .env(WAIT_FOR_STATUS_ENV, "1")
            .stdout(Stdio::from(log))
            .stderr(Stdio::from(log_err))
            .pre_exec(|| {
                libc::setsid();
                Ok(())
            })
            .spawn()
    }
    .context("failed to spawn rootless Podman prep worker")?;
    let running = PodmanPrepStatus::running(child.id(), log_path);
    status::write_running_unless_terminal(&status_path, &running).map(|_| ())
}

pub(in crate::guest_init) fn run_prep_to_status() -> Result<()> {
    let env_contract = LoftdEnv::from_process_env()?;
    if !env_contract.containers_storage {
        return Ok(());
    }
    let (uid, gid) = env_contract.require_host_identity()?;
    let identity = DevIdentity::new(uid, gid, PathBuf::from("fish"));
    let status_path = PathBuf::from(PODMAN_STATUS_PATH);
    let log_path = PathBuf::from(PODMAN_LOG_PATH);
    let pid = std::process::id();
    if std::env::var(WAIT_FOR_STATUS_ENV).as_deref() == Ok("1") {
        wait_for_parent_running_status(&status_path, pid)?;
    } else {
        let running = PodmanPrepStatus::running(pid, log_path);
        status::write_running_unless_terminal(&status_path, &running)?;
    }

    match run_prep(&identity, &env_contract) {
        // Ready means kernel/userns, idmap, storage, and config prep completed.
        // The long-lived Podman API socket is started lazily by the Docker
        // compatibility wait path, not by root prep.
        Ok(()) => status::mark_ready_for_pid(&status_path, pid),
        Err(err) => {
            let message = format!("{err:#}");
            let _ = append_log(&message);
            status::mark_failed_for_pid(&status_path, pid, message)
        }
    }
}

pub(in crate::guest_init) fn run_prep(
    identity: &DevIdentity,
    env_contract: &LoftdEnv,
) -> Result<()> {
    if !env_contract.containers_storage {
        return Ok(());
    }
    if !process::is_root() {
        bail!("internal container storage bootstrap must run as root");
    }
    for tool in required_tools(env_contract.container_store_backend) {
        command::require_on_path(tool)?;
    }
    let tool_paths = PodmanToolPaths::discover()?;
    crate::guest_init::components::podman::kernel::prepare()?;
    crate::guest_init::components::podman::idmap::prepare(identity)?;
    crate::guest_init::components::podman::storage::bootstrap(identity, env_contract, &tool_paths)
}

fn required_tools(container_store_backend: ContainerStoreBackend) -> &'static [&'static str] {
    match container_store_backend {
        ContainerStoreBackend::Bind => &["podman"],
        ContainerStoreBackend::RawDisk => &["blkid", "mount", "findmnt", "podman"],
    }
}

fn wait_for_parent_running_status(status_path: &Path, pid: u32) -> Result<()> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let current = status::read_status(status_path)?;
        if current.state == PodmanPrepState::Running && current.pid == Some(pid) {
            return Ok(());
        }
        if std::time::Instant::now() >= deadline {
            bail!(
                "timed out waiting for parent to publish podman prep running status for pid {pid}"
            );
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

fn append_log(message: &str) -> Result<()> {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(PODMAN_LOG_PATH)?;
    writeln!(file, "{message}")?;
    Ok(())
}

#[cfg(test)]
#[path = "root_tests.rs"]
mod tests;
