use anyhow::{Context, Result, anyhow, bail};
use std::fs;
use std::path::{Path, PathBuf};

use crate::guest_init::components::env::{
    CONTAINERS_STORE_ENV, ContainerStoreBackend, DEFAULT_SHELL, ENTER_AS_ROOT_ENV, LoftdEnv,
    NIX_REMOTE_URI,
};
use crate::guest_init::components::home::identity::{DevIdentity, validate_host_identity};
use crate::guest_init::runtime::session::{self, ManagedSessionConfig};
use crate::guest_init::{command, process, profile};

const HOST_UID_ENV: &str = "LOFTD_HOST_UID";
const HOST_GID_ENV: &str = "LOFTD_HOST_GID";
const LEGACY_HOST_UID_ENV: &str = "AGENTBOX_HOST_UID";
const LEGACY_HOST_GID_ENV: &str = "AGENTBOX_HOST_GID";
const LEGACY_ENTER_AS_ROOT_ENV: &str = "AGENTBOX_ENTER_AS_ROOT";
const LEGACY_NIX_OVERLAY_ENV: &str = "AGENTBOX_LIBKRUN_NIX_OVERLAY";
const LEGACY_CONTAINERS_STORAGE_ENV: &str = "AGENTBOX_LIBKRUN_CONTAINERS_STORAGE";
const LEGACY_USE_PASST_ENV: &str = "AGENTBOX_LIBKRUN_USE_PASST";
const SESSION_MANAGED_ENV: &str = "LOFTD_SESSION_MANAGED";
const ATTACH_PORT_ENV: &str = "LOFTD_ATTACH_PORT";
const ATTACH_PROTOCOL_VERSION_ENV: &str = "LOFTD_ATTACH_PROTOCOL_VERSION";
const PREPARED_ROOT_TARGETS: &[&str] = &[
    "/workspace",
    "/home/dev/.codex",
    "/home/dev/.omp",
    "/home/dev/.pi",
    "/home/dev/.cargo",
    "/home/dev/.cache/sccache",
];

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::guest_init) enum LoftdEnterOperation {
    ReadEnv,
    ValidatePreparedRootPaths,
    EnsureTmpTmpfs,
    EnsureTunDevice,
    ResolveIdentity,
    DeriveShellEnvironment,
    ExportShellEnvironment,
    MaterializeHome,
    MaterializeAllocatorPreload,
    RestrictDmesg,
    StartNixPrep,
    StartPodmanPrep,
    ExportNixRemote,
    EnsureNofileFloor,
    ClearProfileEnvBeforeExec,
    ReportProfileBeforeExec,
    DropAndExec,
    RunManagedSession,
}

#[cfg(test)]
pub(in crate::guest_init) fn planned_enter_operations() -> Vec<LoftdEnterOperation> {
    vec![
        LoftdEnterOperation::ReadEnv,
        LoftdEnterOperation::ValidatePreparedRootPaths,
        LoftdEnterOperation::EnsureTmpTmpfs,
        LoftdEnterOperation::EnsureTunDevice,
        LoftdEnterOperation::ResolveIdentity,
        LoftdEnterOperation::DeriveShellEnvironment,
        LoftdEnterOperation::ExportShellEnvironment,
        LoftdEnterOperation::MaterializeHome,
        LoftdEnterOperation::MaterializeAllocatorPreload,
        LoftdEnterOperation::RestrictDmesg,
        LoftdEnterOperation::StartNixPrep,
        LoftdEnterOperation::StartPodmanPrep,
        LoftdEnterOperation::ExportNixRemote,
        LoftdEnterOperation::EnsureNofileFloor,
        LoftdEnterOperation::ClearProfileEnvBeforeExec,
        LoftdEnterOperation::ReportProfileBeforeExec,
        LoftdEnterOperation::DropAndExec,
        LoftdEnterOperation::RunManagedSession,
    ]
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct EnterEnv {
    enter_as_root: bool,
    host_uid: Option<u32>,
    host_gid: Option<u32>,
    loftd: LoftdEnv,
    managed_session: Option<ManagedSessionConfig>,
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
    debug_breadcrumb("validate-prepared-root starting");
    profiler.measure_result("validate-prepared-root", || {
        validate_prepared_root_paths(&prepared_root_targets())
    })?;
    debug_breadcrumb("validate-prepared-root complete");
    profiler.measure_result("ensure-tmp-tmpfs", ensure_tmp_tmpfs_mounted)?;
    profiler.measure_result("ensure-tun-device", || {
        crate::guest_init::components::rootless::kernel::prepare_tun_device()
    })?;
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
    profiler.measure_result("ensure-nofile-floor", process::ensure_nofile_floor)?;

    profile::clear_guest_profile_env();
    profiler.report_before_exec()?;
    let drop_to_identity = should_drop_to_identity(process::is_root(), env_contract.enter_as_root);
    if let Some(managed_session) = env_contract.managed_session {
        debug_breadcrumb("managed session starting");
        return session::run(&command, &identity, drop_to_identity, managed_session);
    }
    debug_breadcrumb("final exec handoff starting");
    if drop_to_identity {
        process::drop_to_identity_and_exec(&identity, &command)
    } else {
        process::exec_command(&command)
    }
}

impl EnterEnv {
    fn from_env(env: &impl EnvSource) -> Result<Self> {
        Ok(Self {
            enter_as_root: env_flag_any(env, ENTER_AS_ROOT_ENV, LEGACY_ENTER_AS_ROOT_ENV),
            host_uid: parse_optional_u32_any(env, HOST_UID_ENV, LEGACY_HOST_UID_ENV)?,
            host_gid: parse_optional_u32_any(env, HOST_GID_ENV, LEGACY_HOST_GID_ENV)?,
            loftd: loftd_env_from(env)?,
            managed_session: managed_session_from_env(env)?,
        })
    }
}

fn managed_session_from_env(env: &impl EnvSource) -> Result<Option<ManagedSessionConfig>> {
    if !env_flag(env, SESSION_MANAGED_ENV) {
        return Ok(None);
    }
    Ok(Some(ManagedSessionConfig {
        port: parse_required_u32(env, ATTACH_PORT_ENV)?,
        protocol_version: parse_required_u16(env, ATTACH_PROTOCOL_VERSION_ENV)?,
    }))
}

fn parse_required_u32(env: &impl EnvSource, name: &str) -> Result<u32> {
    env.var(name)
        .ok_or_else(|| anyhow!("{name} is required when {SESSION_MANAGED_ENV}=1"))?
        .parse::<u32>()
        .with_context(|| format!("invalid numeric value in {name}"))
}

fn parse_required_u16(env: &impl EnvSource, name: &str) -> Result<u16> {
    env.var(name)
        .ok_or_else(|| anyhow!("{name} is required when {SESSION_MANAGED_ENV}=1"))?
        .parse::<u16>()
        .with_context(|| format!("invalid numeric value in {name}"))
}

fn loftd_env_from(env: &impl EnvSource) -> Result<LoftdEnv> {
    Ok(LoftdEnv {
        nix_overlay: env_flag_any(env, "LOFTD_NIX_OVERLAY", LEGACY_NIX_OVERLAY_ENV),
        nix_host_overlay: env_flag(env, "LOFTD_NIX_HOST_OVERLAY"),
        containers_storage: env_flag_any(
            env,
            "LOFTD_CONTAINERS_STORAGE",
            LEGACY_CONTAINERS_STORAGE_ENV,
        ),
        container_store_backend: ContainerStoreBackend::from_optional_env_value(
            env.var(CONTAINERS_STORE_ENV),
        )?,
        use_passt: env_flag_any(env, "LOFTD_USE_PASST", LEGACY_USE_PASST_ENV),
        enter_as_root: env_flag_any(env, ENTER_AS_ROOT_ENV, LEGACY_ENTER_AS_ROOT_ENV),
        host_uid: parse_optional_u32_any(env, HOST_UID_ENV, LEGACY_HOST_UID_ENV)?,
        host_gid: parse_optional_u32_any(env, HOST_GID_ENV, LEGACY_HOST_GID_ENV)?,
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

fn prepared_root_targets() -> Vec<&'static str> {
    PREPARED_ROOT_TARGETS.to_vec()
}

fn env_flag_any(env: &impl EnvSource, primary: &str, legacy: &str) -> bool {
    env_flag(env, primary) || env_flag(env, legacy)
}

fn env_flag(env: &impl EnvSource, name: &str) -> bool {
    env.var(name).as_deref() == Some("1")
}

fn parse_optional_u32_any(
    env: &impl EnvSource,
    primary: &str,
    legacy: &str,
) -> Result<Option<u32>> {
    parse_optional_u32(env, primary)?
        .map_or_else(|| parse_optional_u32(env, legacy), |value| Ok(Some(value)))
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
    if is_root {
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
enum TmpTmpfsPlan {
    Mount,
    Remount,
}

fn validate_prepared_root_paths(paths: &[&str]) -> Result<()> {
    for path in paths {
        validate_prepared_root_path(Path::new(path))?;
    }
    Ok(())
}

fn validate_prepared_root_path(path: &Path) -> Result<()> {
    if path.exists() && path.is_dir() {
        return Ok(());
    }
    if path.exists() {
        bail!(
            "loftd prepared-root path '{}' exists but is not a directory",
            path.display()
        );
    }
    bail!(
        "loftd prepared-root path '{}' is missing; host prepared-root bind grafting failed",
        path.display()
    );
}

fn ensure_tmp_tmpfs_mounted() -> Result<()> {
    let tmp = Path::new("/tmp");
    fs::create_dir_all(tmp).context("failed to create loftd /tmp mount target")?;
    let mounts = fs::read_to_string("/proc/mounts").context("failed to read /proc/mounts")?;
    match tmp_tmpfs_plan(&mounts, tmp)? {
        TmpTmpfsPlan::Mount => mount_tmp_tmpfs(tmp),
        TmpTmpfsPlan::Remount => remount_tmp_tmpfs(tmp),
    }
}

fn tmp_tmpfs_plan(mounts: &str, target: &Path) -> Result<TmpTmpfsPlan> {
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
            if source == "tmpfs" && fs_type == "tmpfs" {
                return Ok(TmpTmpfsPlan::Remount);
            }
            bail!(
                "loftd /tmp target {target} is already mounted from {source} as {fs_type}, not tmpfs"
            );
        }
    }
    Ok(TmpTmpfsPlan::Mount)
}

fn mount_tmp_tmpfs(target: &Path) -> Result<()> {
    command::run(
        "mount",
        &[
            "-t",
            "tmpfs",
            "tmpfs",
            "-o",
            "rw,exec,mode=1777",
            &target.display().to_string(),
        ],
    )
    .context("failed to mount loftd /tmp tmpfs")
}

fn remount_tmp_tmpfs(target: &Path) -> Result<()> {
    command::run(
        "mount",
        &[
            "-o",
            "remount,rw,exec,mode=1777",
            &target.display().to_string(),
        ],
    )
    .context("failed to remount loftd /tmp tmpfs")
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
    fn planned_loftd_enter_validates_prepared_root_before_identity_drop_and_exec() {
        let operations = planned_enter_operations();
        let pos = |op| {
            operations
                .iter()
                .position(|candidate| candidate == &op)
                .expect("operation should exist")
        };

        assert!(
            pos(LoftdEnterOperation::ValidatePreparedRootPaths)
                < pos(LoftdEnterOperation::ResolveIdentity)
        );
        assert!(
            pos(LoftdEnterOperation::ValidatePreparedRootPaths)
                < pos(LoftdEnterOperation::EnsureTmpTmpfs)
        );
        assert!(
            pos(LoftdEnterOperation::ValidatePreparedRootPaths)
                < pos(LoftdEnterOperation::EnsureTunDevice)
        );
        assert!(
            pos(LoftdEnterOperation::EnsureTmpTmpfs) < pos(LoftdEnterOperation::ResolveIdentity)
        );
        assert!(
            pos(LoftdEnterOperation::EnsureTunDevice) < pos(LoftdEnterOperation::ResolveIdentity)
        );
        assert!(
            pos(LoftdEnterOperation::ValidatePreparedRootPaths)
                < pos(LoftdEnterOperation::StartNixPrep)
        );
        assert!(
            pos(LoftdEnterOperation::ResolveIdentity) < pos(LoftdEnterOperation::StartPodmanPrep)
        );
        assert!(pos(LoftdEnterOperation::StartNixPrep) < pos(LoftdEnterOperation::StartPodmanPrep));
        assert!(
            pos(LoftdEnterOperation::ExportNixRemote) < pos(LoftdEnterOperation::EnsureNofileFloor)
        );
        assert!(
            pos(LoftdEnterOperation::EnsureNofileFloor) < pos(LoftdEnterOperation::DropAndExec)
        );
        assert!(pos(LoftdEnterOperation::StartPodmanPrep) < pos(LoftdEnterOperation::DropAndExec));
        assert!(pos(LoftdEnterOperation::ExportNixRemote) < pos(LoftdEnterOperation::DropAndExec));
    }

    #[test]
    fn loftd_env_no_longer_requires_bind_mount_env() {
        let parsed = EnterEnv::from_env(&env(&[])).expect("prepared-root env should parse");

        assert!(!parsed.enter_as_root);
        assert_eq!(parsed.host_uid, None);
        assert_eq!(parsed.host_gid, None);
    }

    #[test]
    fn loftd_env_accepts_host_disk_contract_names() {
        let env = EnterEnv::from_env(&env(&[
            ("LOFTD_NIX_OVERLAY", "1"),
            ("LOFTD_NIX_HOST_OVERLAY", "1"),
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
        assert!(env.loftd.nix_host_overlay);
        assert!(env.loftd.containers_storage);
        assert_eq!(
            env.loftd.container_store_backend,
            ContainerStoreBackend::RawDisk
        );
        assert!(!env.loftd.use_passt);
        assert_eq!(env.loftd.nix_disk_id, "loftd-nix");
        assert_eq!(env.loftd.nix_disk_label, "LOFTD_NIX");
        assert_eq!(env.loftd.containers_disk_id, "loftd-containers");
        assert_eq!(env.loftd.containers_disk_label, "LOFTD_CONTAINERS");
        assert_eq!(env.loftd.host_uid, Some(1000));
        assert_eq!(env.loftd.host_gid, Some(1001));
    }

    #[test]
    fn loftd_env_accepts_current_host_agentbox_compat_names() {
        let env = EnterEnv::from_env(&env(&[
            ("AGENTBOX_LIBKRUN_NIX_OVERLAY", "1"),
            ("AGENTBOX_LIBKRUN_CONTAINERS_STORAGE", "1"),
            ("AGENTBOX_LIBKRUN_USE_PASST", "1"),
            ("AGENTBOX_ENTER_AS_ROOT", "1"),
            ("AGENTBOX_HOST_UID", "2000"),
            ("AGENTBOX_HOST_GID", "2001"),
        ]))
        .expect("compat env should parse");

        assert!(env.enter_as_root);
        assert!(env.loftd.nix_overlay);
        assert!(env.loftd.containers_storage);
        assert_eq!(
            env.loftd.container_store_backend,
            ContainerStoreBackend::RawDisk
        );
        assert!(env.loftd.use_passt);
        assert_eq!(env.host_uid, Some(2000));
        assert_eq!(env.host_gid, Some(2001));
    }

    #[test]
    fn loftd_env_rejects_bind_container_store_backend() {
        let err = EnterEnv::from_env(&env(&[
            ("LOFTD_CONTAINERS_STORAGE", "1"),
            (CONTAINERS_STORE_ENV, "bind"),
        ]))
        .expect_err("bind backend should fail");

        assert!(err.to_string().contains(CONTAINERS_STORE_ENV));
    }

    #[test]
    fn loftd_env_rejects_invalid_container_store_backend() {
        let err = EnterEnv::from_env(&env(&[(CONTAINERS_STORE_ENV, "overlay")]))
            .expect_err("invalid backend should fail");

        assert!(err.to_string().contains(CONTAINERS_STORE_ENV));
    }

    #[test]
    fn prepared_root_targets_exclude_raw_disk_container_store_mount() {
        let _raw = EnterEnv::from_env(&env(&[
            ("LOFTD_CONTAINERS_STORAGE", "1"),
            (CONTAINERS_STORE_ENV, "raw-disk"),
        ]))
        .expect("raw env should parse");

        assert!(
            !prepared_root_targets()
                .contains(&crate::guest_init::components::disk::containers::MOUNT_POINT)
        );
    }

    #[test]
    fn loftd_identity_requires_host_ids_when_root_drops_to_dev() {
        let missing_ids_env = EnterEnv::from_env(&env(&[])).expect("env should parse");
        let err = resolve_identity(
            &["fish".to_owned(), "-l".to_owned()],
            &missing_ids_env,
            true,
            0,
            0,
        )
        .expect_err("host ids are required");
        assert!(err.to_string().contains(HOST_UID_ENV));

        let valid_env = EnterEnv::from_env(&env(&[(HOST_UID_ENV, "1000"), (HOST_GID_ENV, "1001")]))
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
    fn loftd_root_shell_still_materializes_non_root_dev_identity() {
        let root_env = EnterEnv::from_env(&env(&[
            (crate::guest_init::components::env::ENTER_AS_ROOT_ENV, "1"),
            (HOST_UID_ENV, "1000"),
            (HOST_GID_ENV, "1001"),
        ]))
        .expect("env should parse");

        let identity =
            resolve_identity(&["fish".to_owned(), "-l".to_owned()], &root_env, true, 0, 0)
                .expect("root shell identity should resolve");

        assert_eq!(identity.uid, 1000);
        assert_eq!(identity.gid, 1001);
    }

    #[test]
    fn prepared_root_validation_requires_existing_directories() {
        let dir = tempfile::tempdir().expect("tempdir");
        let workspace = dir.path().join("workspace");
        fs::create_dir_all(&workspace).expect("workspace dir");

        validate_prepared_root_paths(&[workspace.to_str().expect("utf8")])
            .expect("existing directory should pass");

        let missing = dir.path().join("missing");
        let err = validate_prepared_root_paths(&[missing.to_str().expect("utf8")])
            .expect_err("missing prepared-root path should fail");
        assert!(format!("{err:#}").contains("prepared-root path"));

        let file = dir.path().join("file");
        fs::write(&file, "not a dir").expect("file");
        let err = validate_prepared_root_paths(&[file.to_str().expect("utf8")])
            .expect_err("file prepared-root path should fail");
        assert!(format!("{err:#}").contains("not a directory"));
    }

    #[test]
    fn tmp_tmpfs_plan_mounts_absent_tmp_and_remounts_existing_tmpfs() {
        assert_eq!(
            tmp_tmpfs_plan("", Path::new("/tmp")).expect("missing /tmp mount should mount"),
            TmpTmpfsPlan::Mount
        );
        assert_eq!(
            tmp_tmpfs_plan("tmpfs /tmp tmpfs rw,nosuid,nodev 0 0\n", Path::new("/tmp"))
                .expect("existing tmpfs should remount with loftd options"),
            TmpTmpfsPlan::Remount
        );
    }

    #[test]
    fn tmp_tmpfs_plan_rejects_non_tmpfs_mountpoint() {
        let err = tmp_tmpfs_plan("other /tmp virtiofs rw 0 0\n", Path::new("/tmp"))
            .expect_err("non-tmpfs /tmp mount should fail");

        assert!(format!("{err:#}").contains("not tmpfs"));
    }
}
