use anyhow::{Context, Result, anyhow, bail};
use clap::Parser;
use std::ffi::CString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

mod cli;

use cli::{GuestInitCli, GuestInitCommand};

const DEFAULT_SHELL: &str = "fish";
const DEV_USER: &str = "dev";
const DEV_HOME: &str = "/home/dev";
const WORKSPACE_TAG_ENV: &str = "LOFTD_WORKSPACE_TAG";
const WORKSPACE_TARGET_ENV: &str = "LOFTD_WORKSPACE_TARGET";
const HOST_UID_ENV: &str = "LOFTD_HOST_UID";
const HOST_GID_ENV: &str = "LOFTD_HOST_GID";
const ENTER_AS_ROOT_ENV: &str = "LOFTD_ENTER_AS_ROOT";
const NIX_OVERLAY_ENV: &str = "LOFTD_NIX_OVERLAY";
const NIX_DISK_ID_ENV: &str = "LOFTD_NIX_DISK_ID";
const NIX_DISK_LABEL_ENV: &str = "LOFTD_NIX_DISK_LABEL";
const CONTAINERS_STORAGE_ENV: &str = "LOFTD_CONTAINERS_STORAGE";
const CONTAINERS_DISK_ID_ENV: &str = "LOFTD_CONTAINERS_DISK_ID";
const CONTAINERS_DISK_LABEL_ENV: &str = "LOFTD_CONTAINERS_DISK_LABEL";
const NIX_REMOTE_URI: &str = "unix:///nix/var/nix/daemon-socket/socket";
const RUN_DIR: &str = "/run/loftd";
const CONTAINERS_MOUNT: &str = "/home/dev/.local/share/containers";

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::guest_init) enum EnterOperation {
    ReadEnv,
    MountWorkspace,
    ResolveIdentity,
    DeriveShellEnvironment,
    ExportShellEnvironment,
    MaterializeHome,
    StartNixPrep,
    StartPodmanPrep,
    ExportNixRemote,
    DropAndExec,
}

#[cfg(test)]
pub(in crate::guest_init) fn planned_enter_operations() -> Vec<EnterOperation> {
    vec![
        EnterOperation::ReadEnv,
        EnterOperation::MountWorkspace,
        EnterOperation::ResolveIdentity,
        EnterOperation::DeriveShellEnvironment,
        EnterOperation::ExportShellEnvironment,
        EnterOperation::MaterializeHome,
        EnterOperation::StartNixPrep,
        EnterOperation::StartPodmanPrep,
        EnterOperation::ExportNixRemote,
        EnterOperation::DropAndExec,
    ]
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct EnterEnv {
    workspace_tag: String,
    workspace_target: PathBuf,
    enter_as_root: bool,
    host_uid: Option<u32>,
    host_gid: Option<u32>,
    nix_overlay: bool,
    nix_disk_id: String,
    nix_disk_label: String,
    containers_storage: bool,
    containers_disk_id: String,
    containers_disk_label: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DevIdentity {
    uid: u32,
    gid: u32,
    home: PathBuf,
    shell: PathBuf,
}

impl DevIdentity {
    fn new(uid: u32, gid: u32, shell: PathBuf) -> Self {
        Self {
            uid,
            gid,
            home: PathBuf::from(DEV_HOME),
            shell,
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

pub(crate) fn entrypoint() -> anyhow::Result<()> {
    match GuestInitCli::parse().command {
        GuestInitCommand::Enter(command) => enter(command.resolved_command()),
    }
}

fn enter(command: Vec<String>) -> Result<()> {
    let env_contract = EnterEnv::from_env(&ProcessEnv)?;
    ensure_workspace_mounted(&env_contract.workspace_tag, &env_contract.workspace_target)?;
    let identity = resolve_identity(&command, &env_contract, is_root(), uid(), gid())?;
    export_shell_environment(&identity, env_contract.containers_storage);
    materialize_home(&identity)?;
    start_nix_prep(&env_contract)?;
    start_podman_prep(&identity, &env_contract)?;
    if env_contract.nix_overlay {
        // SAFETY: guest-init mutates the process environment during single-threaded bootstrap
        // immediately before exec so the shell inherits NIX_REMOTE.
        unsafe { std::env::set_var("NIX_REMOTE", NIX_REMOTE_URI) };
    }

    if should_drop_to_identity(is_root(), env_contract.enter_as_root) {
        drop_to_identity_and_exec(&identity, &command)
    } else {
        exec_command(&command)
    }
}

impl EnterEnv {
    fn from_env(env: &impl EnvSource) -> Result<Self> {
        let workspace_tag = env
            .var(WORKSPACE_TAG_ENV)
            .ok_or_else(|| anyhow!("{WORKSPACE_TAG_ENV} is required for loftd enter"))?;
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
            nix_overlay: env.var(NIX_OVERLAY_ENV).as_deref() == Some("1"),
            nix_disk_id: env
                .var(NIX_DISK_ID_ENV)
                .unwrap_or_else(|| "loftd-nix".to_owned()),
            nix_disk_label: env
                .var(NIX_DISK_LABEL_ENV)
                .unwrap_or_else(|| "LOFTD_NIX".to_owned()),
            containers_storage: env.var(CONTAINERS_STORAGE_ENV).as_deref() == Some("1"),
            containers_disk_id: env
                .var(CONTAINERS_DISK_ID_ENV)
                .unwrap_or_else(|| "loftd-containers".to_owned()),
            containers_disk_label: env
                .var(CONTAINERS_DISK_LABEL_ENV)
                .unwrap_or_else(|| "LOFTD_CONTAINERS".to_owned()),
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

fn ensure_workspace_mounted(tag: &str, target: &Path) -> Result<()> {
    if target.exists() && !target.is_dir() {
        bail!(
            "loftd workspace target '{}' exists but is not a directory",
            target.display()
        );
    }
    fs::create_dir_all(target).with_context(|| {
        format!(
            "failed to create loftd workspace target '{}'",
            target.display()
        )
    })?;
    let mounts = fs::read_to_string("/proc/mounts").context("failed to read /proc/mounts")?;
    match workspace_mount_plan(&mounts, tag, target)? {
        WorkspaceMountPlan::AlreadyMounted => Ok(()),
        WorkspaceMountPlan::Mount => run("mount", &["-t", "virtiofs", tag, &path_string(target)?])
            .with_context(|| format!("failed to mount loftd workspace virtiofs tag {tag}")),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum WorkspaceMountPlan {
    AlreadyMounted,
    Mount,
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
                "loftd workspace target {target} is already mounted from {source} as {fs_type}, not virtiofs tag {tag}"
            );
        }
    }
    Ok(WorkspaceMountPlan::Mount)
}

fn resolve_identity(
    command: &[String],
    env: &EnterEnv,
    is_root: bool,
    current_uid: u32,
    current_gid: u32,
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
        Ok(DevIdentity::new(current_uid, current_gid, shell))
    }
}

fn should_drop_to_identity(is_root: bool, enter_as_root: bool) -> bool {
    is_root && !enter_as_root
}

fn validate_host_identity(uid: u32, gid: u32) -> Result<()> {
    if uid == 0 || gid == 0 {
        bail!("loftd host UID/GID must identify the non-root dev user, got {uid}:{gid}");
    }
    Ok(())
}

fn resolve_shell(command: &[String]) -> PathBuf {
    let shell = command.first().map(String::as_str).unwrap_or(DEFAULT_SHELL);
    if shell.contains('/') {
        PathBuf::from(shell)
    } else {
        find_on_path(shell).unwrap_or_else(|| PathBuf::from(shell))
    }
}

fn export_shell_environment(identity: &DevIdentity, containers_storage: bool) {
    let home = identity.home.display().to_string();
    let tmpdir = identity.home.join(".cache/tmp").display().to_string();
    let vars = [
        ("USER", DEV_USER.to_owned()),
        ("HOME", home.clone()),
        ("SHELL", identity.shell.display().to_string()),
        ("XDG_CONFIG_HOME", format!("{home}/.config")),
        ("XDG_DATA_HOME", format!("{home}/.local/share")),
        ("XDG_STATE_HOME", format!("{home}/.local/state")),
        ("XDG_CACHE_HOME", format!("{home}/.cache")),
        ("TMPDIR", tmpdir),
    ];
    for (key, value) in vars {
        // SAFETY: guest-init builds the shell environment during single-threaded bootstrap.
        unsafe { std::env::set_var(key, value) };
    }
    if containers_storage {
        // SAFETY: same single-threaded bootstrap environment mutation.
        unsafe { std::env::set_var("XDG_RUNTIME_DIR", format!("/run/user/{}", identity.uid)) };
    }
}

fn materialize_home(identity: &DevIdentity) -> Result<()> {
    if !is_root() {
        return Ok(());
    }
    let passwd = read_without_dev(Path::new("/etc/passwd"))?;
    let group = read_without_dev(Path::new("/etc/group"))?;
    write_file(
        Path::new("/etc/passwd"),
        &format!(
            "{passwd}{DEV_USER}:x:{}:{}:dev user:{}:{}\n",
            identity.uid,
            identity.gid,
            identity.home.display(),
            identity.shell.display()
        ),
        0o644,
    )?;
    write_file(
        Path::new("/etc/group"),
        &format!("{group}{DEV_USER}:x:{}:\n", identity.gid),
        0o644,
    )?;
    for path in [
        identity.home.clone(),
        identity.home.join(".local"),
        identity.home.join(".local/share"),
        identity.home.join(".local/state"),
        identity.home.join(".cache"),
        identity.home.join(".cache/tmp"),
        identity.home.join(".config"),
    ] {
        fs::create_dir_all(&path)
            .with_context(|| format!("failed to create {}", path.display()))?;
        chown(&path, identity.uid, identity.gid)?;
    }
    Ok(())
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

fn start_nix_prep(env: &EnterEnv) -> Result<()> {
    if !env.nix_overlay {
        return Ok(());
    }
    let disk = find_labeled_disk(&env.nix_disk_label, &env.nix_disk_id)
        .context("loftd /nix btrfs disk not found")?;
    let run_dir = Path::new(RUN_DIR);
    let disk_mount = run_dir.join("nix-disk");
    let upper_dir = disk_mount.join("upper");
    let work_dir = disk_mount.join("work");
    fs::create_dir_all(run_dir).context("failed to create loftd run dir")?;
    fs::create_dir_all(&disk_mount)
        .with_context(|| format!("failed to create {}", disk_mount.display()))?;
    mount_if_needed(&disk, &disk_mount, "loftd /nix btrfs disk")?;
    fs::create_dir_all(&upper_dir)
        .with_context(|| format!("failed to create {}", upper_dir.display()))?;
    fs::create_dir_all(&work_dir)
        .with_context(|| format!("failed to create {}", work_dir.display()))?;
    let options = format!(
        "lowerdir=/nix,upperdir={},workdir={}",
        upper_dir.display(),
        work_dir.display()
    );
    if !is_exact_mount(Path::new("/nix"))? {
        run(
            "mount",
            &["-t", "overlay", "overlay", "-o", &options, "/nix"],
        )
        .context("failed to mount loftd overlay at /nix")?;
    }
    fs::create_dir_all("/nix/var/nix/daemon-socket")
        .context("failed to create nix daemon socket dir")?;
    let _ = Command::new("nix-daemon")
        .arg("--daemon")
        .spawn()
        .context("failed to start nix-daemon")?;
    Ok(())
}

fn start_podman_prep(identity: &DevIdentity, env: &EnterEnv) -> Result<()> {
    if !env.containers_storage {
        return Ok(());
    }
    let mount = Path::new(CONTAINERS_MOUNT);
    fs::create_dir_all(mount).with_context(|| format!("failed to create {}", mount.display()))?;
    let disk = find_labeled_disk(&env.containers_disk_label, &env.containers_disk_id)
        .context("loftd container-store btrfs disk not found")?;
    mount_if_needed(&disk, mount, "loftd container-store btrfs disk")?;
    chown(mount, identity.uid, identity.gid)
}

fn find_labeled_disk(label: &str, disk_id: &str) -> Result<PathBuf> {
    if let Some(path) = output_trimmed("blkid", &["-L", label])? {
        return Ok(PathBuf::from(path));
    }
    let output = Command::new("sh")
        .arg("-c")
        .arg(format!(
            "for candidate in /dev/disk/by-id/*{disk_id}* /dev/vd? /dev/sd? /dev/xvd? /dev/nvme?n? /dev/pmem?; do [ -e \"$candidate\" ] && printf '%s\\n' \"$candidate\"; done"
        ))
        .output()
        .context("failed to enumerate disk candidates")?;
    for candidate in String::from_utf8_lossy(&output.stdout).lines() {
        if output_trimmed("blkid", &["-o", "value", "-s", "LABEL", candidate])?.as_deref()
            == Some(label)
        {
            return Ok(PathBuf::from(candidate));
        }
    }
    bail!("no btrfs disk with label {label} and id {disk_id}")
}

fn mount_if_needed(source: &Path, target: &Path, label: &str) -> Result<()> {
    if is_exact_mount(target)? {
        return Ok(());
    }
    run(
        "mount",
        &["-t", "btrfs", &path_string(source)?, &path_string(target)?],
    )
    .with_context(|| format!("failed to mount {label}"))
}

fn is_exact_mount(target: &Path) -> Result<bool> {
    let target = path_string(target)?;
    let mounts = fs::read_to_string("/proc/mounts").context("failed to read /proc/mounts")?;
    Ok(mounts.lines().any(|line| {
        let mut fields = line.split_whitespace();
        let _source = fields.next();
        fields.next() == Some(target.as_str())
    }))
}

fn run(program: &str, args: &[&str]) -> Result<()> {
    let status = Command::new(program)
        .args(args)
        .status()
        .with_context(|| format!("failed to run {program}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(anyhow!("{program} exited with status {status}"))
    }
}

fn output_trimmed(program: &str, args: &[&str]) -> Result<Option<String>> {
    let output = Command::new(program)
        .args(args)
        .stderr(Stdio::null())
        .output()
        .with_context(|| format!("failed to run {program}"))?;
    if !output.status.success() {
        return Ok(None);
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    Ok((!text.is_empty()).then_some(text))
}

fn find_on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|candidate| is_executable(candidate))
}

fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    path.metadata()
        .map(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

fn exec_command(command: &[String]) -> Result<()> {
    if command.is_empty() {
        return Err(anyhow!("cannot exec an empty command"));
    }
    execvp(command)
}

fn drop_to_identity_and_exec(identity: &DevIdentity, command: &[String]) -> Result<()> {
    if command.is_empty() {
        return Err(anyhow!("cannot exec an empty command"));
    }

    let clear_groups_rc = unsafe { libc::setgroups(0, std::ptr::null()) };
    if clear_groups_rc != 0 {
        return Err(std::io::Error::last_os_error())
            .context("failed to clear supplementary groups");
    }
    if unsafe { libc::setgid(identity.gid) } != 0 {
        return Err(std::io::Error::last_os_error())
            .with_context(|| format!("failed to set gid {}", identity.gid));
    }
    if unsafe { libc::setuid(identity.uid) } != 0 {
        return Err(std::io::Error::last_os_error())
            .with_context(|| format!("failed to set uid {}", identity.uid));
    }

    execvp(command)
}

fn execvp(command: &[String]) -> Result<()> {
    let c_strings = command
        .iter()
        .map(|arg| CString::new(arg.as_str()))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let mut argv = c_strings
        .iter()
        .map(|arg| arg.as_ptr())
        .collect::<Vec<*const libc::c_char>>();
    argv.push(std::ptr::null());

    unsafe {
        libc::execvp(c_strings[0].as_ptr(), argv.as_ptr());
    }
    Err(std::io::Error::last_os_error()).with_context(|| format!("failed to exec {}", command[0]))
}

fn uid() -> u32 {
    unsafe { libc::getuid() }
}

fn gid() -> u32 {
    unsafe { libc::getgid() }
}

fn is_root() -> bool {
    uid() == 0
}

fn write_file(path: &Path, contents: &str, mode: u32) -> Result<()> {
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let tmp = path.with_extension(format!("tmp.{}", std::process::id()));
    {
        let mut file = fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&tmp)
            .with_context(|| format!("failed to write {}", tmp.display()))?;
        file.write_all(contents.as_bytes())?;
        file.sync_all()?;
    }
    fs::set_permissions(&tmp, fs::Permissions::from_mode(mode))?;
    fs::rename(&tmp, path).with_context(|| {
        format!(
            "failed to replace {} with staged {}",
            path.display(),
            tmp.display()
        )
    })
}

fn chown(path: &Path, uid: u32, gid: u32) -> Result<()> {
    let c_path = CString::new(path.as_os_str().as_encoded_bytes())?;
    let rc = unsafe { libc::chown(c_path.as_ptr(), uid, gid) };
    if rc == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
            .with_context(|| format!("failed to chown {} to {uid}:{gid}", path.display()))
    }
}

fn path_string(path: &Path) -> Result<String> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| anyhow!("path is not valid UTF-8: {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[derive(Default)]
    struct TestEnv(HashMap<&'static str, &'static str>);

    impl TestEnv {
        fn with(mut self, name: &'static str, value: &'static str) -> Self {
            self.0.insert(name, value);
            self
        }
    }

    impl EnvSource for TestEnv {
        fn var(&self, name: &str) -> Option<String> {
            self.0.get(name).map(|value| (*value).to_owned())
        }
    }

    #[test]
    fn env_parser_requires_loftd_workspace_and_identity_names() {
        let env = TestEnv::default()
            .with(WORKSPACE_TAG_ENV, "loftd-workspace")
            .with(WORKSPACE_TARGET_ENV, "/workspace")
            .with(HOST_UID_ENV, "1000")
            .with(HOST_GID_ENV, "1001")
            .with(NIX_OVERLAY_ENV, "1")
            .with(NIX_DISK_ID_ENV, "loftd-nix")
            .with(NIX_DISK_LABEL_ENV, "LOFTD_NIX")
            .with(CONTAINERS_STORAGE_ENV, "1")
            .with(CONTAINERS_DISK_ID_ENV, "loftd-containers")
            .with(CONTAINERS_DISK_LABEL_ENV, "LOFTD_CONTAINERS")
            .with("AGENTBOX_HOST_UID", "2000");

        let parsed = EnterEnv::from_env(&env).expect("env should parse");

        assert_eq!(parsed.workspace_tag, "loftd-workspace");
        assert_eq!(parsed.workspace_target, Path::new("/workspace"));
        assert_eq!(parsed.host_uid, Some(1000));
        assert_eq!(parsed.host_gid, Some(1001));
        assert_eq!(parsed.nix_disk_id, "loftd-nix");
        assert_eq!(parsed.containers_disk_id, "loftd-containers");
    }

    #[test]
    fn env_parser_rejects_missing_or_invalid_values() {
        assert!(EnterEnv::from_env(&TestEnv::default()).is_err());
        let err = EnterEnv::from_env(
            &TestEnv::default()
                .with(WORKSPACE_TAG_ENV, "loftd-workspace")
                .with(WORKSPACE_TARGET_ENV, "relative"),
        )
        .expect_err("relative workspace target should fail");
        assert!(err.to_string().contains("absolute"));
        let err = EnterEnv::from_env(
            &TestEnv::default()
                .with(WORKSPACE_TAG_ENV, "loftd-workspace")
                .with(HOST_UID_ENV, "not-a-number"),
        )
        .expect_err("numeric value should fail");
        assert!(err.to_string().contains(HOST_UID_ENV));
    }

    #[test]
    fn planned_operations_mount_workspace_before_identity_drop_and_cache_prep_before_exec() {
        let operations = planned_enter_operations();
        let mount = operations
            .iter()
            .position(|op| op == &EnterOperation::MountWorkspace)
            .expect("mount operation");
        let resolve = operations
            .iter()
            .position(|op| op == &EnterOperation::ResolveIdentity)
            .expect("resolve operation");
        let nix = operations
            .iter()
            .position(|op| op == &EnterOperation::StartNixPrep)
            .expect("nix prep operation");
        let podman = operations
            .iter()
            .position(|op| op == &EnterOperation::StartPodmanPrep)
            .expect("podman prep operation");
        let exec = operations
            .iter()
            .position(|op| op == &EnterOperation::DropAndExec)
            .expect("exec operation");

        assert!(mount < resolve);
        assert!(nix < exec);
        assert!(podman < exec);
    }

    #[test]
    fn workspace_mount_plan_reuses_matching_virtiofs_mount() {
        let plan = workspace_mount_plan(
            "loftd-workspace /workspace virtiofs rw 0 0\n",
            "loftd-workspace",
            Path::new("/workspace"),
        )
        .expect("plan should resolve");

        assert_eq!(plan, WorkspaceMountPlan::AlreadyMounted);
    }

    #[test]
    fn identity_requires_non_root_host_ids_when_dropping_from_root() {
        let env = EnterEnv {
            workspace_tag: "loftd-workspace".to_owned(),
            workspace_target: PathBuf::from("/workspace"),
            enter_as_root: false,
            host_uid: Some(1000),
            host_gid: Some(1001),
            nix_overlay: false,
            nix_disk_id: "loftd-nix".to_owned(),
            nix_disk_label: "LOFTD_NIX".to_owned(),
            containers_storage: false,
            containers_disk_id: "loftd-containers".to_owned(),
            containers_disk_label: "LOFTD_CONTAINERS".to_owned(),
        };

        let identity = resolve_identity(&["fish".to_owned()], &env, true, 0, 0)
            .expect("identity should resolve");

        assert_eq!(identity.uid, 1000);
        assert_eq!(identity.gid, 1001);
    }
}
