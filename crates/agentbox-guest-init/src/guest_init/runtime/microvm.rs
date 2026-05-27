use anyhow::{Context, Result, anyhow, bail};
use std::fs;
use std::path::{Path, PathBuf};

use crate::guest_init::cli::{MicrovmCommand, MicrovmSubcommand};
use crate::guest_init::components::env::{DEFAULT_SHELL, ENTER_AS_ROOT_ENV};
use crate::guest_init::components::home::identity::{DevIdentity, validate_host_identity};
use crate::guest_init::{command, process, profile};

const WORKSPACE_TAG_ENV: &str = "AGENTBOX_MICROVM_WORKSPACE_TAG";
const WORKSPACE_TARGET_ENV: &str = "AGENTBOX_MICROVM_WORKSPACE_TARGET";
const HOST_UID_ENV: &str = "AGENTBOX_HOST_UID";
const HOST_GID_ENV: &str = "AGENTBOX_HOST_GID";

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::guest_init) enum MicrovmEnterOperation {
    ReadEnv,
    MountWorkspace,
    ResolveIdentity,
    DeriveShellEnvironment,
    ExportShellEnvironment,
    MaterializeHome,
    MaterializeAllocatorPreload,
    RestrictDmesg,
    ClearProfileEnvBeforeExec,
    ReportProfileBeforeExec,
    DropAndExec,
}

#[cfg(test)]
pub(in crate::guest_init) fn planned_enter_operations() -> Vec<MicrovmEnterOperation> {
    vec![
        MicrovmEnterOperation::ReadEnv,
        MicrovmEnterOperation::MountWorkspace,
        MicrovmEnterOperation::ResolveIdentity,
        MicrovmEnterOperation::DeriveShellEnvironment,
        MicrovmEnterOperation::ExportShellEnvironment,
        MicrovmEnterOperation::MaterializeHome,
        MicrovmEnterOperation::MaterializeAllocatorPreload,
        MicrovmEnterOperation::RestrictDmesg,
        MicrovmEnterOperation::ClearProfileEnvBeforeExec,
        MicrovmEnterOperation::ReportProfileBeforeExec,
        MicrovmEnterOperation::DropAndExec,
    ]
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MicrovmEnv {
    workspace_tag: String,
    workspace_target: PathBuf,
    enter_as_root: bool,
    host_uid: Option<u32>,
    host_gid: Option<u32>,
}

trait EnvSource {
    fn var(&self, name: &str) -> Option<String>;
}

struct ProcessEnv;

impl EnvSource for ProcessEnv {
    fn var(&self, name: &str) -> Option<String> {
        std::env::var(name).ok().filter(|value| !value.is_empty())
    }
}

pub(in crate::guest_init) fn run(command: MicrovmCommand) -> Result<()> {
    match command.command {
        MicrovmSubcommand::Enter(enter) => enter_microvm(enter.resolved_command()),
    }
}

fn enter_microvm(command: Vec<String>) -> Result<()> {
    let mut profiler = profile::GuestProfiler::from_process_env("microvm enter");
    let env_contract = profiler.measure_result("read-env", || MicrovmEnv::from_env(&ProcessEnv))?;
    profiler.measure_result("mount-workspace", || {
        ensure_workspace_mounted(&env_contract.workspace_tag, &env_contract.workspace_target)
    })?;
    let identity = profiler.measure_result("resolve-identity", || {
        resolve_identity(
            &command,
            &env_contract,
            process::is_root(),
            process::uid(),
            process::gid(),
        )
    })?;
    let shell_env = profiler.measure("derive-shell-env", || {
        crate::guest_init::components::shell::env::derive(&identity, false)
    });
    profiler.measure("export-shell-env", || {
        crate::guest_init::components::shell::env::export(&shell_env)
    });
    profiler.measure_result("materialize-home", || {
        crate::guest_init::components::home::root::materialize(&identity)
    })?;
    profiler.measure_result("materialize-allocator-preload", || {
        crate::guest_init::components::hardening::allocator::ensure_from_env_if_root(
            process::is_root(),
        )
    })?;
    profiler.measure_result("restrict-dmesg", || {
        if process::is_root() {
            crate::guest_init::components::hardening::dmesg::restrict()?;
        }
        Ok(())
    })?;

    profile::clear_guest_profile_env();
    profiler.report_before_exec()?;
    if should_drop_to_identity(process::is_root(), env_contract.enter_as_root) {
        process::drop_to_identity_and_exec(&identity, &command)
    } else {
        process::exec_command(&command)
    }
}

impl MicrovmEnv {
    fn from_env(env: &impl EnvSource) -> Result<Self> {
        let workspace_tag = env
            .var(WORKSPACE_TAG_ENV)
            .ok_or_else(|| anyhow!("{WORKSPACE_TAG_ENV} is required for microvm enter"))?;
        let workspace_target = env
            .var(WORKSPACE_TARGET_ENV)
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/workspace"));
        if !workspace_target.is_absolute() {
            bail!("{WORKSPACE_TARGET_ENV} must be an absolute path");
        }
        Ok(Self {
            workspace_tag,
            workspace_target,
            enter_as_root: env.var(ENTER_AS_ROOT_ENV).as_deref() == Some("1"),
            host_uid: parse_optional_u32(env, HOST_UID_ENV)?,
            host_gid: parse_optional_u32(env, HOST_GID_ENV)?,
        })
    }
}

fn parse_optional_u32(env: &impl EnvSource, name: &str) -> Result<Option<u32>> {
    env.var(name)
        .map(|value| {
            value
                .parse::<u32>()
                .with_context(|| format!("invalid numeric value in {name}"))
        })
        .transpose()
}

fn resolve_identity(
    command: &[String],
    env: &MicrovmEnv,
    is_root: bool,
    uid: u32,
    gid: u32,
) -> Result<DevIdentity> {
    let shell = resolve_shell(command);
    if should_drop_to_identity(is_root, env.enter_as_root) {
        let uid = env
            .host_uid
            .ok_or_else(|| anyhow!("{HOST_UID_ENV} is required for microvm enter"))?;
        let gid = env
            .host_gid
            .ok_or_else(|| anyhow!("{HOST_GID_ENV} is required for microvm enter"))?;
        validate_host_identity(uid, gid)?;
        Ok(DevIdentity::new(uid, gid, shell))
    } else {
        Ok(DevIdentity::new(uid, gid, shell))
    }
}

fn should_drop_to_identity(is_root: bool, enter_as_root: bool) -> bool {
    is_root && !enter_as_root
}

fn resolve_shell(command: &[String]) -> PathBuf {
    let shell = command.first().map(String::as_str).unwrap_or(DEFAULT_SHELL);
    if shell.contains('/') {
        PathBuf::from(shell)
    } else {
        command::find_on_path(shell).unwrap_or_else(|| PathBuf::from(shell))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum WorkspaceMountPlan {
    AlreadyMounted,
    Mount,
}

fn ensure_workspace_mounted(tag: &str, target: &Path) -> Result<()> {
    if !target.is_absolute() {
        bail!(
            "microvm workspace target '{}' must be absolute",
            target.display()
        );
    }
    if target.exists() && !target.is_dir() {
        bail!(
            "microvm workspace target '{}' exists but is not a directory",
            target.display()
        );
    }
    fs::create_dir_all(target).with_context(|| {
        format!(
            "failed to create microvm workspace target '{}'",
            target.display()
        )
    })?;
    let mounts = fs::read_to_string("/proc/mounts").context("failed to read /proc/mounts")?;
    match workspace_mount_plan(&mounts, tag, target)? {
        WorkspaceMountPlan::AlreadyMounted => Ok(()),
        WorkspaceMountPlan::Mount => mount_workspace(tag, target),
    }
}

fn workspace_mount_plan(mounts: &str, tag: &str, target: &Path) -> Result<WorkspaceMountPlan> {
    let target = target.display().to_string();
    for line in mounts.lines() {
        let mut fields = line.split_whitespace();
        let Some(source) = fields.next() else {
            continue;
        };
        let Some(mountpoint) = fields.next() else {
            continue;
        };
        let Some(fs_type) = fields.next() else {
            continue;
        };
        if mountpoint == target {
            if source == tag && fs_type == "virtiofs" {
                return Ok(WorkspaceMountPlan::AlreadyMounted);
            }
            bail!(
                "microvm workspace target {target} is already mounted from {source} as {fs_type}, not virtiofs tag {tag}"
            );
        }
    }
    Ok(WorkspaceMountPlan::Mount)
}

fn mount_workspace(tag: &str, target: &Path) -> Result<()> {
    command::run(
        "mount",
        &["-t", "virtiofs", tag, &target.display().to_string()],
    )
    .with_context(|| format!("failed to mount microvm workspace virtiofs tag {tag}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[derive(Default)]
    struct TestEnv(HashMap<&'static str, &'static str>);

    impl EnvSource for TestEnv {
        fn var(&self, name: &str) -> Option<String> {
            self.0.get(name).map(|value| (*value).to_owned())
        }
    }

    fn env(vars: &[(&'static str, &'static str)]) -> TestEnv {
        TestEnv(vars.iter().copied().collect())
    }

    #[test]
    fn planned_microvm_enter_mounts_workspace_before_identity_drop_and_exec() {
        let operations = planned_enter_operations();
        let mount = operations
            .iter()
            .position(|op| op == &MicrovmEnterOperation::MountWorkspace)
            .expect("mount operation should exist");
        let drop = operations
            .iter()
            .position(|op| op == &MicrovmEnterOperation::DropAndExec)
            .expect("drop/exec operation should exist");

        assert!(mount < drop);
        let names = format!("{operations:?}").to_lowercase();
        assert!(!names.contains("podman"));
        assert!(!names.contains("nix"));
    }

    #[test]
    fn microvm_env_requires_workspace_tag_and_absolute_target() {
        assert!(MicrovmEnv::from_env(&env(&[])).is_err());
        assert!(
            MicrovmEnv::from_env(&env(&[(WORKSPACE_TAG_ENV, "agentbox-workspace")]))
                .expect("target should default")
                .workspace_target
                .is_absolute()
        );
        assert!(
            MicrovmEnv::from_env(&env(&[
                (WORKSPACE_TAG_ENV, "agentbox-workspace"),
                (WORKSPACE_TARGET_ENV, "workspace"),
            ]))
            .is_err()
        );
    }

    #[test]
    fn microvm_identity_requires_host_ids_when_root_drops_to_dev() {
        let missing_ids_env =
            MicrovmEnv::from_env(&env(&[(WORKSPACE_TAG_ENV, "agentbox-workspace")]))
                .expect("env should parse");
        let err = resolve_identity(
            &["fish".to_owned(), "-l".to_owned()],
            &missing_ids_env,
            true,
            0,
            0,
        )
        .expect_err("host ids are required");
        assert!(err.to_string().contains(HOST_UID_ENV));

        let valid_env = MicrovmEnv::from_env(&env(&[
            (WORKSPACE_TAG_ENV, "agentbox-workspace"),
            (HOST_UID_ENV, "1000"),
            (HOST_GID_ENV, "1001"),
        ]))
        .expect("env should parse");
        let identity = resolve_identity(
            &["fish".to_owned(), "-l".to_owned()],
            &valid_env,
            true,
            0,
            0,
        )
        .expect("identity should resolve");
        assert_eq!(identity.uid, 1000);
        assert_eq!(identity.gid, 1001);
    }

    #[test]
    fn workspace_mount_is_idempotent_for_same_virtiofs_tag() {
        assert_eq!(
            workspace_mount_plan(
                "agentbox-workspace /workspace virtiofs rw 0 0\n",
                "agentbox-workspace",
                Path::new("/workspace"),
            )
            .expect("same tag mount should be accepted"),
            WorkspaceMountPlan::AlreadyMounted
        );
        assert_eq!(
            workspace_mount_plan("", "agentbox-workspace", Path::new("/workspace"))
                .expect("missing mount should mount"),
            WorkspaceMountPlan::Mount
        );
        assert!(
            workspace_mount_plan(
                "other /workspace virtiofs rw 0 0\n",
                "agentbox-workspace",
                Path::new("/workspace"),
            )
            .is_err()
        );
    }
}
