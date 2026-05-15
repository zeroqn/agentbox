use anyhow::{bail, Context, Result};
use std::fs::OpenOptions;
use std::io::Write;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::guest_init::command;
use crate::guest_init::components::docker::config::{daemon_json, DockerPaths};
use crate::guest_init::components::docker::status::{self, DockerState, DockerStatus};
use crate::guest_init::components::env::{
    LibkrunEnv, DOCKER_PREP_LOG_PATH, DOCKER_PREP_STATUS_PATH, RUN_DIR,
};
use crate::guest_init::components::home::identity::DevIdentity;
use crate::guest_init::{fs, process};

const WAIT_FOR_STATUS_ENV: &str = "AGENTBOX_DOCKER_PREP_WAIT_FOR_STATUS";

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::guest_init) enum DockerPrepOperation {
    WriteRunningStatus,
    EnableUserNamespaces,
    PrepareTun,
    MaterializeSubids,
    InstallIdmapHelpers,
    MountContainerStorage,
    WriteDaemonConfig,
    WriteReadyStatus,
}

#[cfg(test)]
pub(in crate::guest_init) fn planned_operations() -> Vec<DockerPrepOperation> {
    vec![
        DockerPrepOperation::WriteRunningStatus,
        DockerPrepOperation::EnableUserNamespaces,
        DockerPrepOperation::PrepareTun,
        DockerPrepOperation::MaterializeSubids,
        DockerPrepOperation::InstallIdmapHelpers,
        DockerPrepOperation::MountContainerStorage,
        DockerPrepOperation::WriteDaemonConfig,
        DockerPrepOperation::WriteReadyStatus,
    ]
}

pub(in crate::guest_init) fn start_background_prep(env_contract: &LibkrunEnv) -> Result<()> {
    if !env_contract.containers_storage {
        return Ok(());
    }
    if !process::is_root() {
        bail!("rootless Docker root prep must start as root");
    }
    fs::create_dir_all(Path::new(RUN_DIR))?;
    let status_path = PathBuf::from(DOCKER_PREP_STATUS_PATH);
    let log_path = PathBuf::from(DOCKER_PREP_LOG_PATH);
    let current_exe = std::env::current_exe().context("failed to resolve guest-init executable")?;
    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .with_context(|| format!("failed to open {}", log_path.display()))?;
    let log_err = log.try_clone()?;
    let child = unsafe {
        Command::new(current_exe)
            .args(["libkrun", "docker", "prep"])
            .env(WAIT_FOR_STATUS_ENV, "1")
            .stdout(Stdio::from(log))
            .stderr(Stdio::from(log_err))
            .pre_exec(|| {
                libc::setsid();
                Ok(())
            })
            .spawn()
    }
    .context("failed to spawn rootless Docker prep worker")?;
    let running = DockerStatus::running(child.id(), log_path);
    status::write_running_unless_terminal(&status_path, &running).map(|_| ())
}

pub(in crate::guest_init) fn run_prep_to_status() -> Result<()> {
    let env_contract = LibkrunEnv::from_process_env()?;
    if !env_contract.containers_storage {
        return Ok(());
    }
    let (uid, gid) = env_contract.require_host_identity()?;
    let identity = DevIdentity::new(uid, gid, PathBuf::from("fish"));
    let status_path = PathBuf::from(DOCKER_PREP_STATUS_PATH);
    let log_path = PathBuf::from(DOCKER_PREP_LOG_PATH);
    let pid = std::process::id();
    if std::env::var(WAIT_FOR_STATUS_ENV).as_deref() == Ok("1") {
        wait_for_parent_running_status(&status_path, pid)?;
    } else {
        let running = DockerStatus::running(pid, log_path);
        status::write_running_unless_terminal(&status_path, &running)?;
    }

    match run_prep(&identity, &env_contract) {
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
    env_contract: &LibkrunEnv,
) -> Result<()> {
    if !env_contract.containers_storage {
        return Ok(());
    }
    if !process::is_root() {
        bail!("libkrun Docker storage bootstrap must run as root");
    }
    for tool in [
        "blkid",
        "mount",
        "findmnt",
        "docker",
        "dockerd",
        "dockerd-rootless.sh",
        "rootlesskit",
        "newuidmap",
        "newgidmap",
    ] {
        command::require_on_path(tool)?;
    }
    crate::guest_init::components::rootless::kernel::prepare()?;
    crate::guest_init::components::rootless::idmap::prepare(identity)?;
    bootstrap_storage(identity, env_contract)
}

fn bootstrap_storage(identity: &DevIdentity, env_contract: &LibkrunEnv) -> Result<()> {
    crate::guest_init::components::disk::containers::ensure_mounted(
        &env_contract.containers_disk_label,
        &env_contract.containers_disk_id,
    )?;
    let paths = DockerPaths::for_identity(identity);
    crate::guest_init::components::rootless::runtime_dir::ensure_user_runtime_dir(identity)?;
    for path in [
        paths.config_dir.as_path(),
        paths.data_root.as_path(),
        paths.exec_root.as_path(),
        paths.state_root.as_path(),
        paths.runtime_dir.as_path(),
    ] {
        fs::create_dir_all(path)?;
        fs::chown(path, identity.uid, identity.gid)?;
    }
    fs::chmod(&paths.runtime_dir, 0o700)?;
    fs::write_file(&paths.daemon_config, &daemon_json(&paths), 0o644)?;
    fs::chown(&paths.daemon_config, identity.uid, identity.gid)
}

fn wait_for_parent_running_status(status_path: &Path, pid: u32) -> Result<()> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let current = status::read_status(status_path)?;
        if current.state == DockerState::Running && current.pid == Some(pid) {
            return Ok(());
        }
        if std::time::Instant::now() >= deadline {
            bail!(
                "timed out waiting for parent to publish docker prep running status for pid {pid}"
            );
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

fn append_log(message: &str) -> Result<()> {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(DOCKER_PREP_LOG_PATH)?;
    writeln!(file, "{message}")?;
    Ok(())
}

#[cfg(test)]
#[path = "root_tests.rs"]
mod tests;
