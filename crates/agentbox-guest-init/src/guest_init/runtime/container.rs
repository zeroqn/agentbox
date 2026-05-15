use anyhow::{anyhow, bail, Context, Result};
use std::ffi::CString;
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use crate::guest_init::cli::{ContainerCommand, ContainerSubcommand, EnterCommand};
use crate::guest_init::components::env::DEV_USER;
use crate::guest_init::components::home::identity::DevIdentity;
use crate::guest_init::{command, fs as guest_fs, process, profile};

const NSS_WRAPPER_LIB_ENV: &str = "AGENTBOX_NSS_WRAPPER_LIB";
const HOST_UID_ENV: &str = "AGENTBOX_HOST_UID";
const HOST_GID_ENV: &str = "AGENTBOX_HOST_GID";
const DROP_TO_DEV_ENV: &str = "AGENTBOX_KVM_DROP_TO_DEV";
const LIBKRUN_NIX_OVERLAY_ENV: &str = "AGENTBOX_LIBKRUN_NIX_OVERLAY";
const LIBKRUN_CONTAINERS_STORAGE_ENV: &str = "AGENTBOX_LIBKRUN_CONTAINERS_STORAGE";

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::guest_init) enum ContainerEnterOperation {
    ResolveCommand,
    DispatchLibkrunIfRequested,
    StartProfilerAfterLibkrunDispatch,
    DeriveIdentity,
    ExportShellEnvironment,
    MaterializeNssWrapper,
    MaterializeHomeConfig,
    ClearProfileEnvBeforeExec,
    ReportProfileBeforeExec,
    DropAndExec,
}

#[cfg(test)]
pub(in crate::guest_init) fn planned_enter_operations() -> Vec<ContainerEnterOperation> {
    vec![
        ContainerEnterOperation::ResolveCommand,
        ContainerEnterOperation::DispatchLibkrunIfRequested,
        ContainerEnterOperation::StartProfilerAfterLibkrunDispatch,
        ContainerEnterOperation::DeriveIdentity,
        ContainerEnterOperation::ExportShellEnvironment,
        ContainerEnterOperation::MaterializeNssWrapper,
        ContainerEnterOperation::MaterializeHomeConfig,
        ContainerEnterOperation::ClearProfileEnvBeforeExec,
        ContainerEnterOperation::ReportProfileBeforeExec,
        ContainerEnterOperation::DropAndExec,
    ]
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::guest_init) struct ContainerIdentityPlan {
    pub(in crate::guest_init) identity: DevIdentity,
    pub(in crate::guest_init) drop_to_dev: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::guest_init) struct NssWrapperPlan {
    pub(in crate::guest_init) passwd: String,
    pub(in crate::guest_init) group: String,
    pub(in crate::guest_init) ld_preload: String,
}

pub(in crate::guest_init) fn run(command: ContainerCommand) -> Result<()> {
    match command.command {
        ContainerSubcommand::Enter(enter_command) => enter(enter_command),
    }
}

fn enter(command: EnterCommand) -> Result<()> {
    let command = command.resolved_command();
    if should_dispatch_libkrun_from_env() {
        return process::exec_command(&libkrun_dispatch_argv(&command)?);
    }

    let mut profiler = profile::GuestProfiler::from_process_env("container enter");
    let identity_plan = profiler.measure_result("derive-identity", || {
        derive_identity_plan(&command, ProcessIds::current(), &ProcessEnv)
    })?;
    let shell_env = profiler.measure("derive-shell-env", || {
        normal_shell_environment(&identity_plan.identity)
    });
    profiler.measure("export-shell-env", || export_vars(&shell_env));
    let nss_plan = profiler.measure_result("build-nss-wrapper", || {
        build_nss_wrapper_plan(
            Path::new("/etc/passwd"),
            Path::new("/etc/group"),
            &identity_plan.identity,
            &ProcessEnv.var("LD_PRELOAD").unwrap_or_default(),
            ProcessEnv.var(NSS_WRAPPER_LIB_ENV).as_deref(),
        )
    })?;
    let nss_dir = profiler.measure_result("materialize-nss-wrapper", || {
        materialize_nss_wrapper(&nss_plan, identity_plan.drop_to_dev)
    })?;
    profiler.measure("export-nss-wrapper-env", || {
        export_vars(&[
            (
                "NSS_WRAPPER_PASSWD".to_owned(),
                nss_dir.join("passwd").display().to_string(),
            ),
            (
                "NSS_WRAPPER_GROUP".to_owned(),
                nss_dir.join("group").display().to_string(),
            ),
            ("LD_PRELOAD".to_owned(), nss_plan.ld_preload),
        ]);
    });
    profiler.measure_result("materialize-home-config", || {
        materialize_home_config(&identity_plan.identity, identity_plan.drop_to_dev)
    })?;

    if identity_plan.drop_to_dev {
        profiler.measure_result("materialize-dev-identity", || {
            materialize_dev_identity_files(&nss_dir)
        })?;
    }

    profile::clear_guest_profile_env();
    profiler.report_before_exec()?;
    if identity_plan.drop_to_dev {
        process::drop_to_identity_and_exec(&identity_plan.identity, &command)
    } else {
        process::exec_command(&command)
    }
}

fn should_dispatch_libkrun_from_env() -> bool {
    should_dispatch_libkrun(&ProcessEnv)
}

fn should_dispatch_libkrun(env: &impl EnvSource) -> bool {
    env.var(LIBKRUN_NIX_OVERLAY_ENV).as_deref() == Some("1")
        || env.var(LIBKRUN_CONTAINERS_STORAGE_ENV).as_deref() == Some("1")
}

fn libkrun_dispatch_argv(command: &[String]) -> Result<Vec<String>> {
    let current_exe = std::env::current_exe()
        .context("failed to resolve current agentbox-guest-init executable")?
        .display()
        .to_string();
    Ok(libkrun_dispatch_argv_for_exe(&current_exe, command))
}

pub(in crate::guest_init) fn libkrun_dispatch_argv_for_exe(
    exe: &str,
    command: &[String],
) -> Vec<String> {
    let mut argv = vec![
        exe.to_owned(),
        "libkrun".to_owned(),
        "enter".to_owned(),
        "--".to_owned(),
    ];
    argv.extend(command.iter().cloned());
    argv
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::guest_init) struct ProcessIds {
    pub(in crate::guest_init) uid: u32,
    pub(in crate::guest_init) gid: u32,
}

impl ProcessIds {
    fn current() -> Self {
        Self {
            uid: process::uid(),
            gid: process::gid(),
        }
    }
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

fn derive_identity_plan(
    command: &[String],
    ids: ProcessIds,
    env: &impl EnvSource,
) -> Result<ContainerIdentityPlan> {
    let mut dev_uid = ids.uid;
    let mut dev_gid = ids.gid;
    if ids.uid == 0 {
        dev_uid = 1000;
        dev_gid = 1000;
        if let (Some(host_uid), Some(host_gid)) = (env.var(HOST_UID_ENV), env.var(HOST_GID_ENV)) {
            dev_uid = parse_u32_env(HOST_UID_ENV, &host_uid)?;
            dev_gid = parse_u32_env(HOST_GID_ENV, &host_gid)?;
        }
    }

    let interactive_fish_task = command_basename(command).as_deref() == Some("fish")
        && command.get(1).map(String::as_str) == Some("-l");
    let drop_to_dev =
        ids.uid == 0 && (env.var(DROP_TO_DEV_ENV).as_deref() == Some("1") || interactive_fish_task);
    if drop_to_dev && (env.var(HOST_UID_ENV).is_none() || env.var(HOST_GID_ENV).is_none()) {
        bail!(
            "agentbox-guest-init container enter: AGENTBOX_HOST_UID and AGENTBOX_HOST_GID are required for KVM task mode"
        );
    }

    Ok(ContainerIdentityPlan {
        identity: DevIdentity::new(dev_uid, dev_gid, resolve_login_shell()),
        drop_to_dev,
    })
}

fn parse_u32_env(name: &str, value: &str) -> Result<u32> {
    value
        .parse()
        .with_context(|| format!("invalid numeric value in {name}"))
}

fn command_basename(command: &[String]) -> Option<String> {
    let command = command.first()?;
    Path::new(command)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
}

fn resolve_login_shell() -> PathBuf {
    command::find_on_path("fish").unwrap_or_else(|| PathBuf::from("fish"))
}

fn normal_shell_environment(identity: &DevIdentity) -> Vec<(String, String)> {
    let home = identity.home.display().to_string();
    vec![
        ("USER".to_owned(), DEV_USER.to_owned()),
        ("HOME".to_owned(), home.clone()),
        ("SHELL".to_owned(), identity.shell.display().to_string()),
        ("XDG_CONFIG_HOME".to_owned(), format!("{home}/.config")),
        ("XDG_DATA_HOME".to_owned(), format!("{home}/.local/share")),
        ("XDG_STATE_HOME".to_owned(), format!("{home}/.local/state")),
        ("XDG_CACHE_HOME".to_owned(), format!("{home}/.cache")),
        ("TMPDIR".to_owned(), format!("{home}/.cache/tmp")),
    ]
}

fn export_vars(vars: &[(String, String)]) {
    for (key, value) in vars {
        // SAFETY: container entry exports the derived login environment during
        // single-threaded bootstrap immediately before replacing the process.
        unsafe { std::env::set_var(key, value) };
    }
}

fn build_nss_wrapper_plan(
    passwd_path: &Path,
    group_path: &Path,
    identity: &DevIdentity,
    old_ld_preload: &str,
    nss_wrapper_lib: Option<&str>,
) -> Result<NssWrapperPlan> {
    let nss_wrapper_lib = nss_wrapper_lib
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("{NSS_WRAPPER_LIB_ENV} is required for container enter"))?;
    let existing_passwd = read_without_dev(passwd_path)?;
    let existing_group = read_without_dev(group_path)?;
    let passwd = format!(
        "{existing_passwd}{DEV_USER}:x:{}:{}:dev user:{}:{}\n",
        identity.uid,
        identity.gid,
        identity.home.display(),
        identity.shell.display()
    );
    let group = format!("{existing_group}{DEV_USER}:x:{}:\n", identity.gid);
    let ld_preload = if old_ld_preload.is_empty() {
        nss_wrapper_lib.to_owned()
    } else {
        format!("{nss_wrapper_lib}:{old_ld_preload}")
    };
    Ok(NssWrapperPlan {
        passwd,
        group,
        ld_preload,
    })
}

fn read_without_dev(path: &Path) -> Result<String> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(err) => return Err(err).with_context(|| format!("failed to read {}", path.display())),
    };
    let mut out = String::new();
    for line in text.lines() {
        if !line.starts_with("dev:") {
            out.push_str(line);
            out.push('\n');
        }
    }
    Ok(out)
}

fn materialize_nss_wrapper(plan: &NssWrapperPlan, drop_to_dev: bool) -> Result<PathBuf> {
    let dir = PathBuf::from(format!("/tmp/agentbox-nss.{}", std::process::id()));
    guest_fs::create_dir_all(&dir)?;
    if drop_to_dev {
        guest_fs::chmod(&dir, 0o755)?;
    }
    let mode = if drop_to_dev { 0o644 } else { 0o600 };
    guest_fs::write_file(&dir.join("passwd"), &plan.passwd, mode)?;
    guest_fs::write_file(&dir.join("group"), &plan.group, mode)?;
    Ok(dir)
}

fn materialize_dev_identity_files(nss_dir: &Path) -> Result<()> {
    let passwd = fs::read_to_string(nss_dir.join("passwd"))
        .context("failed to read generated NSS passwd file")?;
    let group = fs::read_to_string(nss_dir.join("group"))
        .context("failed to read generated NSS group file")?;
    guest_fs::write_file(Path::new("/etc/passwd"), &passwd, 0o644)
        .context("failed to materialize dynamic dev entry in /etc/passwd")?;
    guest_fs::write_file(Path::new("/etc/group"), &group, 0o644)
        .context("failed to materialize dynamic dev entry in /etc/group")?;
    Ok(())
}

fn materialize_home_config(identity: &DevIdentity, set_ownership: bool) -> Result<()> {
    let home_dirs = [
        identity.home.clone(),
        identity.home.join(".local"),
        identity.home.join(".local/share"),
        identity.home.join(".local/state"),
        identity.home.join(".cache"),
        identity.home.join(".cache/tmp"),
        identity.home.join(".config"),
    ];
    for path in &home_dirs {
        guest_fs::create_dir_all(path)?;
        if set_ownership {
            chown_if_possible(path, identity);
        }
    }

    let config_dir = identity.home.join(".config");
    let data_dir = identity.home.join(".local/share");
    let fish_config_dir = config_dir.join("fish");
    let shadow_root = PathBuf::from(format!("/tmp/agentbox-container.{}", std::process::id()));
    materialize_writable_dir(&config_dir, &shadow_root.join("home-config"))?;
    materialize_writable_dir(&data_dir, &shadow_root.join("home-data"))?;
    materialize_writable_dir(&fish_config_dir, &shadow_root.join("fish-config"))?;

    crate::guest_init::components::shell::fish::materialize_configs_with_ownership(
        identity,
        set_ownership,
    )?;
    if set_ownership {
        for path in [
            config_dir.as_path(),
            data_dir.as_path(),
            identity.home.join(".cache/starship").as_path(),
            identity.home.join(".cache/tmp").as_path(),
            identity.home.join(".cache").as_path(),
        ] {
            chown_if_possible(path, identity);
        }
    }
    Ok(())
}

fn materialize_writable_dir(path: &Path, shadow: &Path) -> Result<()> {
    if !path.exists() {
        guest_fs::create_dir_all(path)?;
        return Ok(());
    }
    if is_symlink(path) || !is_writable(path) {
        if let Err(err) = shadow_writable_dir(path, shadow) {
            eprintln!(
                "agentbox-guest-init container enter: warning: cannot shadow '{}' to writable layer: {err:#}",
                path.display()
            );
        }
    }
    Ok(())
}

fn shadow_writable_dir(path: &Path, shadow: &Path) -> Result<()> {
    if shadow.exists() {
        fs::remove_dir_all(shadow)
            .with_context(|| format!("failed to reset shadow {}", shadow.display()))?;
    }
    guest_fs::create_dir_all(shadow)?;
    copy_dir_contents(path, shadow)?;
    fs::remove_dir_all(path).with_context(|| format!("failed to remove {}", path.display()))?;
    guest_fs::create_dir_all(path)?;
    copy_dir_contents(shadow, path).with_context(|| {
        format!(
            "failed to materialize writable dir '{}' from shadow '{}'",
            path.display(),
            shadow.display()
        )
    })
}

fn copy_dir_contents(source: &Path, target: &Path) -> Result<()> {
    for entry in
        fs::read_dir(source).with_context(|| format!("failed to read {}", source.display()))?
    {
        let entry = entry?;
        let source_path = entry.path();
        let target_path = target.join(entry.file_name());
        let metadata = fs::metadata(&source_path)
            .with_context(|| format!("failed to read metadata for {}", source_path.display()))?;
        if metadata.is_dir() {
            guest_fs::create_dir_all(&target_path)?;
            copy_dir_contents(&source_path, &target_path)?;
        } else if metadata.is_file() {
            fs::copy(&source_path, &target_path).with_context(|| {
                format!(
                    "failed to copy {} to {}",
                    source_path.display(),
                    target_path.display()
                )
            })?;
        }
    }
    Ok(())
}

fn is_symlink(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|metadata| metadata.file_type().is_symlink())
        .unwrap_or(false)
}

fn is_writable(path: &Path) -> bool {
    let Ok(c_path) = CString::new(path.as_os_str().as_bytes()) else {
        return false;
    };
    unsafe { libc::access(c_path.as_ptr(), libc::W_OK) == 0 }
}

fn chown_if_possible(path: &Path, identity: &DevIdentity) {
    if let Err(err) = guest_fs::chown(path, identity.uid, identity.gid) {
        eprintln!(
            "agentbox-guest-init container enter: warning: chown '{}' to {}:{} failed: {err:#}",
            path.display(),
            identity.uid,
            identity.gid
        );
    }
}

#[cfg(test)]
#[path = "container_tests.rs"]
mod tests;
