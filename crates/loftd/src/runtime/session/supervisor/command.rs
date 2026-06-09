//! Host-side libkrun helper command construction and process spawning.

use anyhow::{Context, Result, anyhow};
use std::ffi::OsString;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::logging::INTERNAL_LOG_LEVEL_ENV;
use crate::runtime::host_tools::{RuntimeTool, runtime_tool_program};
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
            .context("failed to resolve loftd executable for libkrun helper")?;
        build_helper_command(&executable, config_path, config, host_profile_enabled)
    })?;
    tracing::debug!(program = ?spec.program, args = ?spec.args, log_level = config.log_level.as_str(), "loftd libkrun helper command constructed");
    let mut command = spec.into_command();
    command
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    let mut child = profiler.measure_result("helper_spawn_process", || {
        command.spawn().with_context(|| {
        format!(
            "failed to start loftd libkrun helper for '{}'; rootless direct-libkrun launches require util-linux unshare plus newuidmap/newgidmap and usable /etc/subuid + /etc/subgid entries; host /nix overlay launches additionally require buildah on PATH for buildah unshare",
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
    config: &LaunchConfig,
    host_profile_enabled: bool,
) -> Result<HelperCommandSpec> {
    build_helper_command_with_buildah_probe(
        executable,
        config_path,
        config,
        host_profile_enabled,
        buildah_available(),
    )
}

fn build_helper_command_with_buildah_probe(
    executable: &Path,
    config_path: &Path,
    config: &LaunchConfig,
    host_profile_enabled: bool,
    buildah_available: bool,
) -> Result<HelperCommandSpec> {
    if config.host_nix_overlay.is_some() {
        return build_buildah_unshare_helper_command(
            executable,
            config_path,
            config.log_level,
            host_profile_enabled,
            buildah_available,
        );
    }

    let launcher = KeepIdLauncher::from_current_system()?;
    tracing::debug!(summary = %launcher.diagnostic_summary(), "loftd libkrun helper keep-id namespace resolved");
    Ok(build_helper_command_with_launcher(
        executable,
        config_path,
        config.log_level,
        host_profile_enabled,
        &launcher,
    ))
}

fn build_buildah_unshare_helper_command(
    executable: &Path,
    config_path: &Path,
    log_level: crate::logging::LogLevel,
    host_profile_enabled: bool,
    buildah_available: bool,
) -> Result<HelperCommandSpec> {
    if !buildah_available {
        return Err(anyhow!(
            "loftd host /nix overlay requires `buildah` on PATH so the VM worker mount/prepared-root/libkrun lifecycle can run inside `buildah unshare`"
        ));
    }

    tracing::debug!("loftd libkrun helper will run inside buildah unshare for host /nix overlay");
    Ok(HelperCommandSpec {
        program: runtime_tool_program(RuntimeTool::Buildah),
        env: helper_env(log_level, host_profile_enabled),
        args: vec![
            OsString::from("unshare"),
            executable.as_os_str().to_os_string(),
            OsString::from("internal"),
            OsString::from(LIBKRUN_ENTER_HELPER_ARG),
            config_path.as_os_str().to_os_string(),
        ],
    })
}

pub(crate) fn build_helper_command_with_launcher(
    executable: &Path,
    config_path: &Path,
    log_level: crate::logging::LogLevel,
    host_profile_enabled: bool,
    launcher: &KeepIdLauncher,
) -> HelperCommandSpec {
    HelperCommandSpec {
        program: launcher.program(),
        env: helper_env(log_level, host_profile_enabled),
        args: launcher.args(executable, LIBKRUN_ENTER_HELPER_ARG, config_path),
    }
}

fn helper_env(
    log_level: crate::logging::LogLevel,
    host_profile_enabled: bool,
) -> Vec<(OsString, OsString)> {
    let mut env = vec![(
        OsString::from(INTERNAL_LOG_LEVEL_ENV),
        OsString::from(log_level.as_str()),
    )];
    if host_profile_enabled {
        env.push((OsString::from(LOFTD_HOST_PROFILE_ENV), OsString::from("1")));
    }
    env
}

fn buildah_available() -> bool {
    let program = runtime_tool_program(RuntimeTool::Buildah);
    let program = program.to_string_lossy();
    program.contains('/') || which_in_path(&program).is_some()
}

fn which_in_path(program: &str) -> Option<std::path::PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(program))
        .find(|candidate| candidate.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::launch::config::{HostNixOverlay, LaunchConfig, NetworkMode};
    use std::path::PathBuf;

    #[test]
    fn host_nix_overlay_helper_launches_through_buildah_unshare_transaction() {
        let mut config = minimal_launch_config();
        config.host_nix_overlay = Some(host_nix_overlay());

        let spec = build_helper_command_with_buildah_probe(
            Path::new("/nix/store/hash-loftd/bin/loftd"),
            Path::new("/tmp/loftd-task/launch.conf"),
            &config,
            true,
            true,
        )
        .expect("buildah helper spec");

        assert_eq!(spec.program, OsString::from("buildah"));
        assert!(spec.env.contains(&(
            OsString::from("LOFTD_INTERNAL_LOG_LEVEL"),
            OsString::from("debug")
        )));
        assert!(
            spec.env
                .contains(&(OsString::from("LOFTD_HOST_PROFILE"), OsString::from("1")))
        );
        let args = spec
            .args
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert_eq!(
            args,
            vec![
                "unshare",
                "/nix/store/hash-loftd/bin/loftd",
                "internal",
                "libkrun-network-enter",
                "/tmp/loftd-task/launch.conf",
            ]
        );
        assert!(!args.contains(&"--map-users".to_owned()));
        assert!(!args.contains(&"--map-groups".to_owned()));
    }

    #[test]
    fn missing_buildah_is_hard_diagnostic_for_host_nix_overlay_helper() {
        let mut config = minimal_launch_config();
        config.host_nix_overlay = Some(host_nix_overlay());

        let err = build_helper_command_with_buildah_probe(
            Path::new("/nix/store/hash-loftd/bin/loftd"),
            Path::new("/tmp/loftd-task/launch.conf"),
            &config,
            false,
            false,
        )
        .expect_err("missing buildah must fail");

        let message = format!("{err:#}");
        assert!(message.contains("requires `buildah` on PATH"));
        assert!(message.contains("buildah unshare"));
    }

    fn minimal_launch_config() -> LaunchConfig {
        LaunchConfig {
            task_rootfs: PathBuf::from("/tmp/rootfs"),
            hostname: "loftd-test".to_owned(),
            mounts: Vec::new(),
            host_nix_overlay: None,
            guest_init_override: None,
            disks: Vec::new(),
            ram_mib: 1024,
            vcpus: 1,
            log_level: crate::logging::LogLevel::Debug,
            network_mode: NetworkMode::Tsi,
            publish: Vec::new(),
            workdir: "/workspace".to_owned(),
            exec_path: "/bin/sh".to_owned(),
            argv: Vec::new(),
            env: Vec::new(),
            guest_config_env: Vec::new(),
            passt_fd: None,
        }
    }

    fn host_nix_overlay() -> HostNixOverlay {
        HostNixOverlay {
            selected_reference: "localhost/loftd:latest".to_owned(),
            image_digest: "sha256:deadbeef".to_owned(),
            digest_key: "sha256-deadbeef".to_owned(),
            lowerdir: PathBuf::from("/cache/rootfs/nix"),
            upperdir: PathBuf::from("/state/nix-overlay/upper"),
            workdir: PathBuf::from("/state/nix-overlay/work"),
            mergeddir: PathBuf::from("/state/nix-overlay/merged"),
        }
    }
}
