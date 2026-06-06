use anyhow::{Context, Result, anyhow, bail};
use std::ffi::OsString;
use std::os::fd::OwnedFd;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

use crate::logging::{self, INTERNAL_LOG_LEVEL_ENV, LogSettings};
use crate::runtime::ffi::{DirectLibkrunLauncher, DynamicLibkrunApi};
use crate::runtime::keep_id::KeepIdLauncher;
use crate::runtime::launch::config::{LaunchConfig, NetworkMode};
use crate::runtime::network::{self, NetworkManagerSession, PasstWorkerSession};
use crate::runtime::prepared_root;

pub(crate) const LIBKRUN_ENTER_HELPER_ARG: &str = "libkrun-network-enter";

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
        .context("failed to resolve loftd executable for keep-id libkrun helper")?;
    let spec = build_helper_command(&executable, config_path, config.log_level)?;
    tracing::debug!(program = ?spec.program, args = ?spec.args, log_level = config.log_level.as_str(), "loftd libkrun keep-id helper command constructed");
    let mut command = spec.into_command();
    command
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    let mut child = command.spawn().with_context(|| {
        format!(
            "failed to start loftd keep-id libkrun helper for '{}'; rootless direct-libkrun launches require util-linux unshare plus newuidmap/newgidmap and usable /etc/subuid + /etc/subgid entries",
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
) -> Result<HelperCommandSpec> {
    let launcher = KeepIdLauncher::from_current_system()?;
    tracing::debug!(summary = %launcher.diagnostic_summary(), "loftd libkrun helper keep-id namespace resolved");
    Ok(build_helper_command_with_launcher(
        executable,
        config_path,
        log_level,
        &launcher,
    ))
}

fn build_helper_command_with_launcher(
    executable: &Path,
    config_path: &Path,
    log_level: crate::logging::LogLevel,
    launcher: &KeepIdLauncher,
) -> HelperCommandSpec {
    HelperCommandSpec {
        program: launcher.program(),
        env: vec![(
            OsString::from(INTERNAL_LOG_LEVEL_ENV),
            OsString::from(log_level.as_str()),
        )],
        args: launcher.args(executable, LIBKRUN_ENTER_HELPER_ARG, config_path),
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
    configure_helper_filesystem_identity(&config)?;
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

fn configure_helper_filesystem_identity(config: &LaunchConfig) -> Result<()> {
    let host_uid = required_guest_config_u32(config, "LOFTD_HOST_UID")?;
    let host_gid = required_guest_config_u32(config, "LOFTD_HOST_GID")?;
    set_filesystem_gid(host_gid)?;
    set_filesystem_uid(host_uid)?;
    tracing::debug!(
        host_uid,
        host_gid,
        "loftd internal: keep-id helper filesystem identity configured"
    );
    Ok(())
}

fn configure_vm_worker_filesystem_identity() -> Result<()> {
    set_filesystem_gid(0)?;
    set_filesystem_uid(0)?;
    tracing::debug!("loftd internal VM worker: namespace-root filesystem identity restored");
    Ok(())
}

fn required_guest_config_u32(config: &LaunchConfig, key: &str) -> Result<u32> {
    let value = config
        .guest_config_env
        .iter()
        .rev()
        .find_map(|(env_key, env_value)| (env_key == key).then_some(env_value))
        .ok_or_else(|| anyhow!("loftd launch config is missing required {key}"))?;
    value
        .parse::<u32>()
        .with_context(|| format!("loftd launch config {key} value '{value}' is not a u32"))
}

fn set_filesystem_uid(uid: u32) -> Result<()> {
    let uid = uid as libc::uid_t;
    // SAFETY: setfsuid changes only the current process filesystem credential.
    unsafe { libc::setfsuid(uid) };
    // SAFETY: uid_t::MAX is treated by Linux as an invalid fsuid probe and returns the current fsuid.
    let current = unsafe { libc::setfsuid(libc::uid_t::MAX) };
    if current < 0 || current as libc::uid_t != uid {
        bail!("failed to set loftd helper filesystem UID to {uid}; current fsuid is {current}");
    }
    Ok(())
}

fn set_filesystem_gid(gid: u32) -> Result<()> {
    let gid = gid as libc::gid_t;
    // SAFETY: setfsgid changes only the current process filesystem credential.
    unsafe { libc::setfsgid(gid) };
    // SAFETY: gid_t::MAX is treated by Linux as an invalid fsgid probe and returns the current fsgid.
    let current = unsafe { libc::setfsgid(libc::gid_t::MAX) };
    if current < 0 || current as libc::gid_t != gid {
        bail!("failed to set loftd helper filesystem GID to {gid}; current fsgid is {current}");
    }
    Ok(())
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
    configure_vm_worker_filesystem_identity()?;
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
    fn helper_filesystem_identity_parser_uses_latest_required_env_values() {
        let mut config = minimal_launch_config();
        config.guest_config_env = vec![
            ("LOFTD_HOST_UID".to_owned(), "image".to_owned()),
            ("LOFTD_HOST_GID".to_owned(), "image".to_owned()),
            ("LOFTD_HOST_UID".to_owned(), "1000".to_owned()),
            ("LOFTD_HOST_GID".to_owned(), "993".to_owned()),
        ];

        assert_eq!(
            required_guest_config_u32(&config, "LOFTD_HOST_UID").unwrap(),
            1000
        );
        assert_eq!(
            required_guest_config_u32(&config, "LOFTD_HOST_GID").unwrap(),
            993
        );
    }

    #[test]
    fn helper_filesystem_identity_parser_rejects_missing_or_invalid_values() {
        let mut config = minimal_launch_config();
        assert!(
            format!(
                "{:#}",
                required_guest_config_u32(&config, "LOFTD_HOST_UID").unwrap_err()
            )
            .contains("missing")
        );
        config.guest_config_env = vec![("LOFTD_HOST_UID".to_owned(), "not-a-uid".to_owned())];
        assert!(
            format!(
                "{:#}",
                required_guest_config_u32(&config, "LOFTD_HOST_UID").unwrap_err()
            )
            .contains("not a u32")
        );
    }

    fn minimal_launch_config() -> LaunchConfig {
        LaunchConfig {
            task_rootfs: PathBuf::from("/tmp/rootfs"),
            hostname: "loftd-test".to_owned(),
            mounts: Vec::new(),
            guest_init_override: None,
            disks: Vec::new(),
            ram_mib: 1024,
            vcpus: 1,
            log_level: crate::logging::LogLevel::Info,
            network_mode: NetworkMode::Tsi,
            workdir: "/workspace".to_owned(),
            exec_path: "/bin/sh".to_owned(),
            argv: Vec::new(),
            env: Vec::new(),
            guest_config_env: Vec::new(),
            passt_socket: None,
        }
    }

    #[test]
    fn libkrun_helper_launches_through_keep_id_unshare() {
        let launcher = KeepIdLauncher::from_parts(
            1000,
            993,
            "dev",
            crate::runtime::keep_id::SubIdRange::new(100_000, 65_536).unwrap(),
            crate::runtime::keep_id::SubIdRange::new(100_000, 65_536).unwrap(),
        )
        .unwrap();
        let spec = build_helper_command_with_launcher(
            Path::new("/nix/store/hash-loftd/bin/loftd"),
            Path::new("/tmp/loftd-task/launch.conf"),
            crate::logging::LogLevel::Debug,
            &launcher,
        );

        assert_eq!(spec.program, OsString::from("unshare"));
        assert_eq!(
            spec.env,
            vec![(
                OsString::from("LOFTD_INTERNAL_LOG_LEVEL"),
                OsString::from("debug")
            )]
        );
        let args = spec
            .args
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert!(args.contains(&"--user".to_owned()));
        assert!(args.contains(&"--mount".to_owned()));
        assert!(args.contains(&"--keep-caps".to_owned()));
        assert!(args.windows(2).any(|pair| pair == ["--setuid", "0"]));
        assert!(args.windows(2).any(|pair| pair == ["--setgid", "0"]));
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--map-users", "1000:1000:1"])
        );
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--map-groups", "993:993:1"])
        );
        assert_eq!(
            &args[args.len() - 4..],
            [
                "/nix/store/hash-loftd/bin/loftd",
                "internal",
                "libkrun-network-enter",
                "/tmp/loftd-task/launch.conf",
            ]
        );
    }
}
