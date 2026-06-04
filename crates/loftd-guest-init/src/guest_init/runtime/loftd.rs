use anyhow::{Context, Result, anyhow, bail};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::guest_init::components::env::{
    DEFAULT_SHELL, ENTER_AS_ROOT_ENV, LoftdEnv, NIX_REMOTE_URI,
};
use crate::guest_init::components::home::identity::{DevIdentity, validate_host_identity};
use crate::guest_init::{command, process, profile};

const WORKSPACE_TAG_ENV: &str = "LOFTD_WORKSPACE_TAG";
const WORKSPACE_TARGET_ENV: &str = "LOFTD_WORKSPACE_TARGET";
const MOUNT_COUNT_ENV: &str = "LOFTD_MOUNT_COUNT";
const HOST_UID_ENV: &str = "LOFTD_HOST_UID";
const HOST_GID_ENV: &str = "LOFTD_HOST_GID";

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::guest_init) enum LoftdEnterOperation {
    ReadEnv,
    MountBindMounts,
    ResolveIdentity,
    DeriveShellEnvironment,
    ExportShellEnvironment,
    MaterializeHome,
    MaterializeAllocatorPreload,
    RestrictDmesg,
    StartNixPrep,
    StartPodmanPrep,
    ExportNixRemote,
    ClearProfileEnvBeforeExec,
    ReportProfileBeforeExec,
    DropAndExec,
}

#[cfg(test)]
pub(in crate::guest_init) fn planned_enter_operations() -> Vec<LoftdEnterOperation> {
    vec![
        LoftdEnterOperation::ReadEnv,
        LoftdEnterOperation::MountBindMounts,
        LoftdEnterOperation::ResolveIdentity,
        LoftdEnterOperation::DeriveShellEnvironment,
        LoftdEnterOperation::ExportShellEnvironment,
        LoftdEnterOperation::MaterializeHome,
        LoftdEnterOperation::MaterializeAllocatorPreload,
        LoftdEnterOperation::RestrictDmesg,
        LoftdEnterOperation::StartNixPrep,
        LoftdEnterOperation::StartPodmanPrep,
        LoftdEnterOperation::ExportNixRemote,
        LoftdEnterOperation::ClearProfileEnvBeforeExec,
        LoftdEnterOperation::ReportProfileBeforeExec,
        LoftdEnterOperation::DropAndExec,
    ]
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct EnterEnv {
    mounts: Vec<BindMount>,
    enter_as_root: bool,
    host_uid: Option<u32>,
    host_gid: Option<u32>,
    loftd: LoftdEnv,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BindMount {
    tag: String,
    target: PathBuf,
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

pub(in crate::guest_init) fn enter(command: Vec<String>) -> Result<()> {
    debug_breadcrumb("enter dispatch reached");
    let mut profiler = profile::GuestProfiler::from_process_env("loftd enter");
    debug_breadcrumb("read-env starting");
    let env_contract = profiler.measure_result("read-env", || EnterEnv::from_env(&ProcessEnv))?;
    debug_breadcrumb("read-env complete");
    debug_breadcrumb("mount-bind-mounts starting");
    profiler.measure_result("mount-bind-mounts", || {
        ensure_bind_mounts_mounted(&env_contract.mounts)
    })?;
    debug_breadcrumb("mount-bind-mounts complete");
    debug_breadcrumb("resolve-identity starting");
    let identity = profiler.measure_result("resolve-identity", || {
        resolve_identity(
            &command,
            &env_contract,
            process::is_root(),
            process::uid(),
            process::gid(),
        )
    })?;
    debug_breadcrumb("resolve-identity complete");
    let shell_env = profiler.measure("derive-shell-env", || {
        crate::guest_init::components::shell::env::derive(
            &identity,
            env_contract.loftd.containers_storage,
        )
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
    profiler.measure_result("configure-passt-dns", || {
        if env_contract.loftd.use_passt && process::is_root() {
            crate::guest_init::components::net::dns::ensure_passt_resolv_conf(Path::new(
                "/etc/resolv.conf",
            ))?;
        }
        Ok(())
    })?;
    profiler.measure_result("start-nix-prep", || {
        crate::guest_init::components::nix::root::start_background_prep(&env_contract.loftd)
    })?;
    profiler.measure_result("start-podman-prep", || {
        crate::guest_init::components::podman::root::start_background_prep(
            &identity,
            &env_contract.loftd,
        )
    })?;
    profiler.measure("export-nix-remote", || {
        if env_contract.loftd.nix_overlay {
            // SAFETY: loftd guest-init mutates the process environment during
            // single-threaded bootstrap before exec so the shell sees NIX_REMOTE.
            unsafe { std::env::set_var("NIX_REMOTE", NIX_REMOTE_URI) };
        }
    });

    profile::clear_guest_profile_env();
    profiler.report_before_exec()?;
    debug_breadcrumb("final exec handoff starting");
    if should_drop_to_identity(process::is_root(), env_contract.enter_as_root) {
        process::drop_to_identity_and_exec(&identity, &command)
    } else {
        process::exec_command(&command)
    }
}

impl EnterEnv {
    fn from_env(env: &impl EnvSource) -> Result<Self> {
        Ok(Self {
            mounts: bind_mounts_from_env(env)?,
            enter_as_root: env.var(ENTER_AS_ROOT_ENV).as_deref() == Some("1"),
            host_uid: parse_optional_u32(env, HOST_UID_ENV)?,
            host_gid: parse_optional_u32(env, HOST_GID_ENV)?,
            loftd: loftd_env_from(env)?,
        })
    }
}

fn bind_mounts_from_env(env: &impl EnvSource) -> Result<Vec<BindMount>> {
    let mounts = if let Some(count) = env.var(MOUNT_COUNT_ENV) {
        let count = count
            .parse::<usize>()
            .with_context(|| format!("invalid numeric value in {MOUNT_COUNT_ENV}"))?;
        let mut mounts = Vec::with_capacity(count);
        for index in 0..count {
            let tag_name = format!("LOFTD_MOUNT_{index}_TAG");
            let target_name = format!("LOFTD_MOUNT_{index}_TARGET");
            let tag = env
                .var(&tag_name)
                .ok_or_else(|| anyhow!("{tag_name} is required for loftd enter"))?;
            let target = env
                .var(&target_name)
                .map(PathBuf::from)
                .ok_or_else(|| anyhow!("{target_name} is required for loftd enter"))?;
            mounts.push(BindMount { tag, target });
        }
        mounts
    } else {
        let tag = env
            .var(WORKSPACE_TAG_ENV)
            .ok_or_else(|| anyhow!("{WORKSPACE_TAG_ENV} is required for loftd enter"))?;
        let target = env
            .var(WORKSPACE_TARGET_ENV)
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/workspace"));
        vec![BindMount { tag, target }]
    };
    validate_bind_mounts(&mounts)?;
    Ok(mounts)
}

fn validate_bind_mounts(mounts: &[BindMount]) -> Result<()> {
    if mounts.is_empty() {
        bail!("loftd enter requires at least one bind mount");
    }
    let mut tags = BTreeSet::new();
    let mut targets = BTreeSet::new();
    for mount in mounts {
        if mount.tag.trim().is_empty() {
            bail!("loftd bind mount tag cannot be empty");
        }
        if !mount.target.is_absolute() {
            bail!(
                "loftd bind mount target '{}' must be absolute",
                mount.target.display()
            );
        }
        if mount.target.to_string_lossy().contains(".config/codex") {
            bail!("loftd bind mounts must not include .config/codex");
        }
        if !tags.insert(mount.tag.as_str()) {
            bail!("loftd bind mount tag '{}' is duplicated", mount.tag);
        }
        let target = mount.target.display().to_string();
        if !targets.insert(target.clone()) {
            bail!("loftd bind mount target '{target}' is duplicated");
        }
    }
    Ok(())
}

fn loftd_env_from(env: &impl EnvSource) -> Result<LoftdEnv> {
    Ok(LoftdEnv {
        nix_overlay: env_flag(env, "LOFTD_NIX_OVERLAY"),
        containers_storage: env_flag(env, "LOFTD_CONTAINERS_STORAGE"),
        use_passt: env_flag(env, "LOFTD_USE_PASST"),
        enter_as_root: env_flag(env, ENTER_AS_ROOT_ENV),
        host_uid: parse_optional_u32(env, HOST_UID_ENV)?,
        host_gid: parse_optional_u32(env, HOST_GID_ENV)?,
        nix_disk_id: env
            .var("LOFTD_NIX_DISK_ID")
            .unwrap_or_else(|| crate::guest_init::components::env::RAW_NIX_DISK_ID.to_owned()),
        nix_disk_label: env
            .var("LOFTD_NIX_DISK_LABEL")
            .unwrap_or_else(|| crate::guest_init::components::env::RAW_NIX_DISK_LABEL.to_owned()),
        containers_disk_id: env.var("LOFTD_CONTAINERS_DISK_ID").unwrap_or_else(|| {
            crate::guest_init::components::env::RAW_CONTAINER_DISK_ID.to_owned()
        }),
        containers_disk_label: env.var("LOFTD_CONTAINERS_DISK_LABEL").unwrap_or_else(|| {
            crate::guest_init::components::env::RAW_CONTAINER_DISK_LABEL.to_owned()
        }),
    })
}

fn env_flag(env: &impl EnvSource, name: &str) -> bool {
    env.var(name).as_deref() == Some("1")
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
    env: &EnterEnv,
    is_root: bool,
    uid: u32,
    gid: u32,
) -> Result<DevIdentity> {
    let shell = resolve_shell(command);
    if should_drop_to_identity(is_root, env.enter_as_root) {
        let uid = env
            .host_uid
            .ok_or_else(|| anyhow!("{HOST_UID_ENV} is required for loftd enter"))?;
        let gid = env
            .host_gid
            .ok_or_else(|| anyhow!("{HOST_GID_ENV} is required for loftd enter"))?;
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
enum BindMountPlan {
    AlreadyMounted,
    Mount,
}

fn ensure_bind_mounts_mounted(bind_mounts: &[BindMount]) -> Result<()> {
    let mounts = fs::read_to_string("/proc/mounts").context("failed to read /proc/mounts")?;
    for bind_mount in bind_mounts {
        ensure_bind_mount_mounted(&mounts, bind_mount)?;
    }
    Ok(())
}

fn ensure_bind_mount_mounted(mounts: &str, bind_mount: &BindMount) -> Result<()> {
    let target = &bind_mount.target;
    if target.exists() && !target.is_dir() {
        bail!(
            "loftd bind mount target '{}' exists but is not a directory",
            target.display()
        );
    }
    fs::create_dir_all(target).with_context(|| {
        format!(
            "failed to create loftd bind mount target '{}'",
            target.display()
        )
    })?;
    match bind_mount_plan(mounts, &bind_mount.tag, target)? {
        BindMountPlan::AlreadyMounted => Ok(()),
        BindMountPlan::Mount => mount_bind_mount(&bind_mount.tag, target),
    }
}

fn bind_mount_plan(mounts: &str, tag: &str, target: &Path) -> Result<BindMountPlan> {
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
                return Ok(BindMountPlan::AlreadyMounted);
            }
            bail!(
                "loftd bind mount target {target} is already mounted from {source} as {fs_type}, not virtiofs tag {tag}"
            );
        }
    }
    Ok(BindMountPlan::Mount)
}

fn mount_bind_mount(tag: &str, target: &Path) -> Result<()> {
    command::run(
        "mount",
        &["-t", "virtiofs", tag, &target.display().to_string()],
    )
    .with_context(|| format!("failed to mount loftd bind virtiofs tag {tag}"))
}

fn debug_breadcrumb(message: &str) {
    if std::env::var("LOFTD_GUEST_DEBUG").ok().as_deref() == Some("1") {
        eprintln!("loftd-guest-init: debug: {message}");
    }
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
    fn planned_loftd_enter_mounts_workspace_before_identity_drop_and_exec() {
        let operations = planned_enter_operations();
        let pos = |op| {
            operations
                .iter()
                .position(|candidate| candidate == &op)
                .expect("operation should exist")
        };

        assert!(
            pos(LoftdEnterOperation::MountBindMounts) < pos(LoftdEnterOperation::ResolveIdentity)
        );
        assert!(pos(LoftdEnterOperation::MountBindMounts) < pos(LoftdEnterOperation::StartNixPrep));
        assert!(
            pos(LoftdEnterOperation::ResolveIdentity) < pos(LoftdEnterOperation::StartPodmanPrep)
        );
        assert!(pos(LoftdEnterOperation::StartNixPrep) < pos(LoftdEnterOperation::StartPodmanPrep));
        assert!(pos(LoftdEnterOperation::StartPodmanPrep) < pos(LoftdEnterOperation::DropAndExec));
        assert!(pos(LoftdEnterOperation::ExportNixRemote) < pos(LoftdEnterOperation::DropAndExec));
    }

    #[test]
    fn loftd_env_requires_workspace_tag_and_absolute_target() {
        assert!(EnterEnv::from_env(&env(&[])).is_err());
        assert!(
            EnterEnv::from_env(&env(&[(WORKSPACE_TAG_ENV, "loftd-workspace")]))
                .expect("target should default")
                .mounts[0]
                .target
                .is_absolute()
        );
        assert!(
            EnterEnv::from_env(&env(&[
                (WORKSPACE_TAG_ENV, "loftd-workspace"),
                (WORKSPACE_TARGET_ENV, "workspace"),
            ]))
            .is_err()
        );
    }

    #[test]
    fn loftd_env_parses_indexed_bind_mount_contract() {
        let parsed = EnterEnv::from_env(&env(&[
            (MOUNT_COUNT_ENV, "5"),
            ("LOFTD_MOUNT_0_TAG", "loftd-workspace"),
            ("LOFTD_MOUNT_0_TARGET", "/workspace"),
            ("LOFTD_MOUNT_1_TAG", "loftd-codex"),
            ("LOFTD_MOUNT_1_TARGET", "/home/dev/.codex"),
            ("LOFTD_MOUNT_2_TAG", "loftd-pi"),
            ("LOFTD_MOUNT_2_TARGET", "/home/dev/.pi"),
            ("LOFTD_MOUNT_3_TAG", "loftd-cargo"),
            ("LOFTD_MOUNT_3_TARGET", "/home/dev/.cargo"),
            ("LOFTD_MOUNT_4_TAG", "loftd-sccache"),
            ("LOFTD_MOUNT_4_TARGET", "/home/dev/.cache/sccache"),
        ]))
        .expect("indexed mounts should parse");

        assert_eq!(parsed.mounts.len(), 5);
        assert_eq!(parsed.mounts[0].tag, "loftd-workspace");
        assert_eq!(parsed.mounts[0].target, Path::new("/workspace"));
        assert_eq!(parsed.mounts[1].tag, "loftd-codex");
        assert_eq!(parsed.mounts[1].target, Path::new("/home/dev/.codex"));
        assert!(
            !parsed
                .mounts
                .iter()
                .any(|mount| mount.target.to_string_lossy().contains(".config/codex"))
        );
    }

    #[test]
    fn loftd_env_rejects_bad_indexed_mount_contracts() {
        assert!(
            EnterEnv::from_env(&env(&[
                (MOUNT_COUNT_ENV, "1"),
                ("LOFTD_MOUNT_0_TAG", "loftd-workspace"),
            ]))
            .is_err()
        );
        assert!(
            EnterEnv::from_env(&env(&[
                (MOUNT_COUNT_ENV, "1"),
                ("LOFTD_MOUNT_0_TAG", "loftd-workspace"),
                ("LOFTD_MOUNT_0_TARGET", "workspace"),
            ]))
            .is_err()
        );
        assert!(
            EnterEnv::from_env(&env(&[
                (MOUNT_COUNT_ENV, "2"),
                ("LOFTD_MOUNT_0_TAG", "loftd-workspace"),
                ("LOFTD_MOUNT_0_TARGET", "/workspace"),
                ("LOFTD_MOUNT_1_TAG", "loftd-codex"),
                ("LOFTD_MOUNT_1_TARGET", "/workspace"),
            ]))
            .is_err()
        );
        assert!(
            EnterEnv::from_env(&env(&[
                (MOUNT_COUNT_ENV, "1"),
                ("LOFTD_MOUNT_0_TAG", "loftd-config-codex"),
                ("LOFTD_MOUNT_0_TARGET", "/home/dev/.config/codex"),
            ]))
            .is_err()
        );
    }

    #[test]
    fn loftd_env_accepts_host_disk_contract_names() {
        let env = EnterEnv::from_env(&env(&[
            (WORKSPACE_TAG_ENV, "loftd-workspace"),
            ("LOFTD_NIX_OVERLAY", "1"),
            ("LOFTD_NIX_DISK_ID", "loftd-nix"),
            ("LOFTD_NIX_DISK_LABEL", "LOFTD_NIX"),
            ("LOFTD_CONTAINERS_STORAGE", "1"),
            ("LOFTD_CONTAINERS_DISK_ID", "loftd-containers"),
            ("LOFTD_CONTAINERS_DISK_LABEL", "LOFTD_CONTAINERS"),
            (HOST_UID_ENV, "1000"),
            (HOST_GID_ENV, "1001"),
        ]))
        .expect("env should parse");

        assert!(env.loftd.nix_overlay);
        assert!(env.loftd.containers_storage);
        assert!(!env.loftd.use_passt);
        assert_eq!(env.loftd.nix_disk_id, "loftd-nix");
        assert_eq!(env.loftd.nix_disk_label, "LOFTD_NIX");
        assert_eq!(env.loftd.containers_disk_id, "loftd-containers");
        assert_eq!(env.loftd.containers_disk_label, "LOFTD_CONTAINERS");
        assert_eq!(env.loftd.host_uid, Some(1000));
        assert_eq!(env.loftd.host_gid, Some(1001));
    }

    #[test]
    fn loftd_identity_requires_host_ids_when_root_drops_to_dev() {
        let missing_ids_env = EnterEnv::from_env(&env(&[(WORKSPACE_TAG_ENV, "loftd-workspace")]))
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

        let valid_env = EnterEnv::from_env(&env(&[
            (WORKSPACE_TAG_ENV, "loftd-workspace"),
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
    fn bind_mount_is_idempotent_for_same_virtiofs_tag() {
        assert_eq!(
            bind_mount_plan(
                "loftd-workspace /workspace virtiofs rw 0 0\n",
                "loftd-workspace",
                Path::new("/workspace"),
            )
            .expect("same tag mount should be accepted"),
            BindMountPlan::AlreadyMounted
        );
        assert_eq!(
            bind_mount_plan("", "loftd-workspace", Path::new("/workspace"))
                .expect("missing mount should mount"),
            BindMountPlan::Mount
        );
        assert!(
            bind_mount_plan(
                "other /workspace virtiofs rw 0 0\n",
                "loftd-workspace",
                Path::new("/workspace"),
            )
            .is_err()
        );
    }
}
