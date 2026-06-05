use anyhow::{Context, Result, anyhow, bail};
use std::ffi::OsString;
use std::os::fd::OwnedFd;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

use crate::logging::{self, INTERNAL_LOG_LEVEL_ENV, LogSettings};
use crate::runtime::ffi::{DirectLibkrunLauncher, DynamicLibkrunApi};
use crate::runtime::launch_config::{LaunchConfig, NetworkMode};
use crate::runtime::network::{self, NetworkManagerSession, PasstWorkerSession};
use crate::runtime::prepared_root;

pub(crate) const LIBKRUN_ENTER_HELPER_ARG: &str = "libkrun-network-enter";
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
        run_helper_process(config, &config_path)
    }
}

fn run_helper_process(config: &LaunchConfig, config_path: &Path) -> Result<ChildStatus> {
    let executable = std::env::current_exe()
        .context("failed to resolve loftd executable for buildah unshare libkrun helper")?;
    let spec = build_helper_command(&executable, config_path, config.log_level);
    tracing::debug!(program = ?spec.program, args = ?spec.args, log_level = config.log_level.as_str(), "loftd libkrun helper command constructed");
    let mut command = spec.into_command();
    command
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    let mut child = command.spawn().with_context(|| {
        format!(
            "failed to start buildah unshare loftd libkrun helper for '{}'; btrfs-snapshot direct-libkrun launches require buildah",
            config_path.display()
        )
    })?;
    tracing::debug!(pid = child.id(), "loftd libkrun helper spawned");
    let status = child
        .wait()
        .context("failed to wait for loftd libkrun helper")?;
    tracing::debug!(?status, "loftd libkrun helper exited");
    Ok(match status.code() {
        Some(code) => ChildStatus::exited(code),
        None => ChildStatus::signaled(),
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HelperCommandSpec {
    program: OsString,
    env: Vec<(OsString, OsString)>,
    args: Vec<OsString>,
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
) -> HelperCommandSpec {
    HelperCommandSpec {
        program: OsString::from(BUILDAH_PROGRAM),
        env: vec![(
            OsString::from(INTERNAL_LOG_LEVEL_ENV),
            OsString::from(log_level.as_str()),
        )],
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
    if logging::helper_pre_config_debug_enabled() {
        eprintln!(
            "loftd internal: libkrun-network-enter starting config={}",
            config_path.display()
        );
    }
    let config = LaunchConfig::read_from(config_path)?;
    logging::init_tracing(&LogSettings::for_internal_helper(config.log_level))?;
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
    let mut worker = VmWorkerGuard::new(fork_vm_worker(
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

struct VmWorkerGuard {
    pid: libc::pid_t,
}

impl VmWorkerGuard {
    fn new(pid: libc::pid_t) -> Self {
        Self { pid }
    }

    fn wait(&mut self) -> Result<i32> {
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

fn fork_vm_worker(
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

fn task_state_dir_from_config_path(config_path: &Path) -> Result<&Path> {
    config_path.parent().ok_or_else(|| {
        anyhow!(
            "loftd launch config '{}' must live inside a task state directory",
            config_path.display()
        )
    })
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
        let err =
            run_internal(vec!["libkrun-network-enter".into()]).expect_err("missing config path");
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
            crate::logging::LogLevel::Debug,
        );

        assert_eq!(spec.program, OsString::from("buildah"));
        assert_eq!(
            spec.env,
            vec![(
                OsString::from("LOFTD_INTERNAL_LOG_LEVEL"),
                OsString::from("debug")
            )]
        );
        assert_eq!(
            spec.args,
            vec![
                OsString::from("unshare"),
                OsString::from("/nix/store/hash-loftd/bin/loftd"),
                OsString::from("internal"),
                OsString::from("libkrun-network-enter"),
                OsString::from("/tmp/loftd-task/launch.conf"),
            ]
        );
    }
}
