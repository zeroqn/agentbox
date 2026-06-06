//! Forked VM child process and direct libkrun entry path.

use anyhow::{Result, bail};
use std::os::fd::OwnedFd;
use std::path::Path;

use crate::runtime::launch::config::{LaunchConfig, NetworkMode};
use crate::runtime::session::supervisor::entry::task_state_dir_from_config_path;
use crate::runtime::session::supervisor::identity;
use crate::runtime::vm::libkrun::{DirectLibkrunLauncher, DynamicLibkrunApi};
use crate::runtime::vm::network::{self, PasstWorkerSession};
use crate::runtime::vm::prepared_root;

pub(crate) struct VmWorkerGuard {
    pid: libc::pid_t,
}

impl VmWorkerGuard {
    pub(crate) fn new(pid: libc::pid_t) -> Self {
        Self { pid }
    }

    pub(crate) fn wait(&mut self) -> Result<i32> {
        let status = network::wait_pid(self.pid)?;
        self.pid = -1;
        Ok(status)
    }
}

impl Drop for VmWorkerGuard {
    fn drop(&mut self) {
        if self.pid > 0 {
            network::cleanup_pid(self.pid);
        }
    }
}

pub(crate) fn fork_vm_worker(
    config_path: &Path,
    holder_pid: libc::pid_t,
    passt_pid_pipe: Option<OwnedFd>,
) -> Result<libc::pid_t> {
    // SAFETY: fork creates an isolated worker process that enters the target netns and exits.
    let pid = unsafe { libc::fork() };
    if pid < 0 {
        bail!(
            "failed to fork loftd VM worker: {}",
            std::io::Error::last_os_error()
        );
    }
    if pid == 0 {
        let result = run_vm_worker(config_path, holder_pid, passt_pid_pipe);
        if let Err(err) = result {
            eprintln!("loftd internal VM worker: {err:#}");
            std::process::exit(1);
        }
        std::process::exit(0);
    }
    Ok(pid)
}

fn run_vm_worker(
    config_path: &Path,
    holder_pid: libc::pid_t,
    passt_pid_pipe: Option<OwnedFd>,
) -> Result<()> {
    let config = LaunchConfig::read_from(config_path)?;
    identity::configure_vm_worker_filesystem_identity()?;
    network::enter_netns(holder_pid)?;
    let task_state_dir = task_state_dir_from_config_path(config_path)?;
    let (config, _passt_session) = match config.network_mode {
        NetworkMode::Tsi => (config, None),
        NetworkMode::Passt => {
            let session = PasstWorkerSession::start(task_state_dir)?;
            if let Some(pipe) = passt_pid_pipe {
                network::write_passt_pid(pipe, session.pid())?;
            }
            (
                config.with_passt_socket(session.socket().to_path_buf()),
                Some(session),
            )
        }
    };
    run_libkrun_in_current_namespace(&config, task_state_dir)
}

fn run_libkrun_in_current_namespace(config: &LaunchConfig, task_state_dir: &Path) -> Result<()> {
    let prepared_root = prepared_root::prepare(config, task_state_dir)?;
    let launch_config = config.with_root_export(prepared_root.root().to_path_buf());
    let guest_config_path = launch_config.write_guest_config_to_rootfs()?;
    tracing::debug!(
        task_state = %task_state_dir.display(),
        source_rootfs = %config.task_rootfs.display(),
        rootfs = %launch_config.task_rootfs.display(),
        guest_config = %guest_config_path.display(),
        prepared_root_bind_count = launch_config.mounts.len(),
        disks = launch_config.disks.len(),
        ram_mib = launch_config.ram_mib,
        vcpus = launch_config.vcpus,
        exec_path = %launch_config.exec_path,
        argv_len = launch_config.argv.len(),
        env_len = launch_config.env.len(),
        guest_config_env_len = launch_config.guest_config_env.len(),
        "loftd internal: launch config loaded"
    );
    tracing::debug!("libkrun API open: begin");
    let api = DynamicLibkrunApi::open_default()?;
    tracing::debug!("libkrun API open: complete");
    DirectLibkrunLauncher::new(api).start_enter(&launch_config)
}
