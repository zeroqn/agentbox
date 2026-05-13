use anyhow::{anyhow, bail, Context, Result};
use std::fs::OpenOptions;
use std::io::Write;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::guest_init::command;
use crate::guest_init::root::home::DevIdentity;
use crate::guest_init::runtime::libkrun::{
    LibkrunEnv, PODMAN_LOG_PATH, PODMAN_STATUS_PATH, RUN_DIR,
};
use crate::guest_init::status::{self, PodmanPrepStatus};
use crate::guest_init::{fs, process};

const SUBID_START: u32 = 100_000;
const SUBID_COUNT: u32 = 65_536;
const WAIT_FOR_STATUS_ENV: &str = "AGENTBOX_PODMAN_PREP_WAIT_FOR_STATUS";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::guest_init) struct PodmanToolPaths {
    conmon: PathBuf,
    crun: PathBuf,
    netavark_dir: PathBuf,
    aardvark_dns_dir: PathBuf,
    pasta_dir: PathBuf,
}

impl PodmanToolPaths {
    fn discover() -> Result<Self> {
        Ok(Self {
            conmon: command::require_on_path("conmon")?,
            crun: command::require_on_path("crun")?,
            netavark_dir: parent_dir(&command::require_on_path("netavark")?)?,
            aardvark_dns_dir: parent_dir(&command::require_on_path("aardvark-dns")?)?,
            pasta_dir: parent_dir(&command::require_on_path("pasta")?)?,
        })
    }

    #[cfg(test)]
    fn fixture() -> Self {
        Self {
            conmon: PathBuf::from("/nix/store/conmon/bin/conmon"),
            crun: PathBuf::from("/nix/store/crun/bin/crun"),
            netavark_dir: PathBuf::from("/nix/store/netavark/bin"),
            aardvark_dns_dir: PathBuf::from("/nix/store/aardvark-dns/bin"),
            pasta_dir: PathBuf::from("/nix/store/passt/bin"),
        }
    }
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::guest_init) enum PodmanPrepOperation {
    WriteRunningStatus,
    EnableUserNamespaces,
    PrepareTun,
    MaterializeSubids,
    InstallIdmapHelpers,
    MountContainerStorage,
    WriteConfig,
    WriteReadyStatus,
}

#[cfg(test)]
pub(in crate::guest_init) fn planned_operations() -> Vec<PodmanPrepOperation> {
    vec![
        PodmanPrepOperation::WriteRunningStatus,
        PodmanPrepOperation::EnableUserNamespaces,
        PodmanPrepOperation::PrepareTun,
        PodmanPrepOperation::MaterializeSubids,
        PodmanPrepOperation::InstallIdmapHelpers,
        PodmanPrepOperation::MountContainerStorage,
        PodmanPrepOperation::WriteConfig,
        PodmanPrepOperation::WriteReadyStatus,
    ]
}

pub(in crate::guest_init) fn start_background_prep(
    _identity: &DevIdentity,
    env_contract: &LibkrunEnv,
) -> Result<()> {
    if !env_contract.containers_storage {
        return Ok(());
    }
    if !process::is_root() {
        bail!("rootless Podman root prep must start as root");
    }
    fs::create_dir_all(Path::new(RUN_DIR))?;
    let status_path = PathBuf::from(PODMAN_STATUS_PATH);
    let log_path = PathBuf::from(PODMAN_LOG_PATH);
    let current_exe = std::env::current_exe().context("failed to resolve guest-init executable")?;
    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .with_context(|| format!("failed to open {}", log_path.display()))?;
    let log_err = log.try_clone()?;
    let child = unsafe {
        Command::new(current_exe)
            .args(["libkrun", "podman", "prep"])
            .env(WAIT_FOR_STATUS_ENV, "1")
            .stdout(Stdio::from(log))
            .stderr(Stdio::from(log_err))
            .pre_exec(|| {
                libc::setsid();
                Ok(())
            })
            .spawn()
    }
    .context("failed to spawn rootless Podman prep worker")?;
    let running = PodmanPrepStatus::running(child.id(), log_path);
    status::write_running_unless_terminal(&status_path, &running).map(|_| ())
}

pub(in crate::guest_init) fn run_prep_to_status() -> Result<()> {
    let env_contract = LibkrunEnv::from_process_env()?;
    if !env_contract.containers_storage {
        return Ok(());
    }
    let (uid, gid) = env_contract.require_host_identity()?;
    let identity = DevIdentity::new(uid, gid, PathBuf::from("fish"));
    let status_path = PathBuf::from(PODMAN_STATUS_PATH);
    let log_path = PathBuf::from(PODMAN_LOG_PATH);
    let pid = std::process::id();
    if std::env::var(WAIT_FOR_STATUS_ENV).as_deref() == Ok("1") {
        wait_for_parent_running_status(&status_path, pid)?;
    } else {
        let running = PodmanPrepStatus::running(pid, log_path);
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
        bail!("libkrun container storage bootstrap must run as root");
    }
    for tool in ["blkid", "mount", "findmnt", "btrfs", "podman"] {
        command::require_on_path(tool)?;
    }
    let tool_paths = PodmanToolPaths::discover()?;
    enable_user_namespaces()?;
    prepare_tun_device()?;
    materialize_subid_files(identity)?;
    install_idmap_helper("newuidmap")?;
    install_idmap_helper("newgidmap")?;
    bootstrap_container_storage(identity, env_contract, &tool_paths)
}

pub(in crate::guest_init) fn storage_conf(identity: &DevIdentity) -> String {
    let graphroot = "/home/dev/.local/share/containers/storage";
    let runroot = format!("/run/user/{}/containers", identity.uid);
    format!(
        r#"[storage]
driver = "btrfs"
graphroot = "{graphroot}"
runroot = "{runroot}"
"#
    )
}

pub(in crate::guest_init) fn containers_conf(paths: &PodmanToolPaths) -> String {
    format!(
        r#"[containers]
cgroups = "disabled"

[engine]
cgroup_manager = "cgroupfs"
events_logger = "file"
runtime = "crun"
conmon_path = ["{}"]
helper_binaries_dir = ["{}", "{}", "{}", "/run/agentbox/idmap-bin"]

[engine.runtimes]
crun = ["{}"]

[network]
network_backend = "netavark"
"#,
        paths.conmon.display(),
        paths.netavark_dir.display(),
        paths.aardvark_dns_dir.display(),
        paths.pasta_dir.display(),
        paths.crun.display()
    )
}

pub(in crate::guest_init) fn registries_conf() -> &'static str {
    r#"[registries.block]
registries = []

[registries.insecure]
registries = []

[registries.search]
registries = ["docker.io"]
"#
}

pub(in crate::guest_init) fn policy_json() -> &'static str {
    r#"{
  "default": [
    {
      "type": "insecureAcceptAnything"
    }
  ],
  "transports": {
    "docker-daemon": {
      "": [
        {
          "type": "insecureAcceptAnything"
        }
      ]
    }
  }
}
"#
}

fn wait_for_parent_running_status(status_path: &Path, pid: u32) -> Result<()> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let current = status::read_status(status_path)?;
        if current.state == crate::guest_init::status::PodmanPrepState::Running
            && current.pid == Some(pid)
        {
            return Ok(());
        }
        if std::time::Instant::now() >= deadline {
            bail!(
                "timed out waiting for parent to publish podman prep running status for pid {pid}"
            );
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

fn parent_dir(path: &Path) -> Result<PathBuf> {
    path.parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| anyhow!("tool path has no parent directory: {}", path.display()))
}

fn enable_user_namespaces() -> Result<()> {
    maybe_raise_sysctl(Path::new("/proc/sys/user/max_user_namespaces"), 28_633)?;
    if Path::new("/proc/sys/kernel/unprivileged_userns_clone").exists() {
        maybe_raise_sysctl(Path::new("/proc/sys/kernel/unprivileged_userns_clone"), 1)?;
    }
    Ok(())
}

fn maybe_raise_sysctl(path: &Path, target: u32) -> Result<()> {
    if !path.exists() {
        bail!(
            "kernel does not expose {}; rootless Podman needs user namespace support",
            path.display()
        );
    }
    let current = std::fs::read_to_string(path)
        .ok()
        .and_then(|text| text.trim().parse::<u32>().ok())
        .unwrap_or(0);
    if current < target {
        std::fs::write(path, format!("{target}\n")).with_context(|| {
            format!(
                "failed to set {}={target} for rootless Podman",
                path.display()
            )
        })?;
    }
    Ok(())
}

fn prepare_tun_device() -> Result<()> {
    let tun = Path::new("/dev/net/tun");
    if !tun.exists() {
        bail!("rootless Podman TUN device is missing at {}; ensure host /dev/net/tun is passed into the libkrun guest", tun.display());
    }
    fs::chmod(tun, 0o666).context("failed to make /dev/net/tun accessible to rootless Podman")
}

fn materialize_subid_files(identity: &DevIdentity) -> Result<()> {
    reject_subid_overlap(0, "root")?;
    reject_subid_overlap(identity.uid, "dev-uid")?;
    reject_subid_overlap(identity.gid, "dev-gid")?;
    for path in [Path::new("/etc/subuid"), Path::new("/etc/subgid")] {
        let existing = std::fs::read_to_string(path).unwrap_or_default();
        let mut contents = String::new();
        for line in existing.lines() {
            if !line.starts_with("dev:") {
                contents.push_str(line);
                contents.push('\n');
            }
        }
        contents.push_str(&format!("dev:{SUBID_START}:{SUBID_COUNT}\n"));
        fs::write_file(path, &contents, 0o644).with_context(|| {
            format!(
                "failed to materialize {} for rootless Podman",
                path.display()
            )
        })?;
    }
    Ok(())
}

fn reject_subid_overlap(candidate: u32, name: &str) -> Result<()> {
    let end = SUBID_START + SUBID_COUNT - 1;
    if (SUBID_START..=end).contains(&candidate) {
        bail!("subordinate ID range {SUBID_START}:{SUBID_COUNT} overlaps {name} id {candidate}");
    }
    Ok(())
}

fn install_idmap_helper(name: &str) -> Result<()> {
    let src = command::require_on_path(name)?;
    let helper_dir = Path::new("/run/agentbox/idmap-bin");
    fs::create_dir_all(helper_dir)?;
    let dst = helper_dir.join(name);
    command::run(
        "install",
        &[
            "-m",
            "4755",
            "-o",
            "0",
            "-g",
            "0",
            path_str(&src)?,
            path_str(&dst)?,
        ],
    )
    .with_context(|| format!("failed to install root-owned setuid {name} helper"))?;
    Ok(())
}

fn bootstrap_container_storage(
    identity: &DevIdentity,
    env_contract: &LibkrunEnv,
    tool_paths: &PodmanToolPaths,
) -> Result<()> {
    let mount = Path::new("/home/dev/.local/share/containers");
    let storage = mount.join("storage");
    let config_dir = Path::new("/home/dev/.config/containers");
    let run_dir = PathBuf::from(format!("/run/user/{}", identity.uid));
    let runroot = run_dir.join("containers");
    fs::create_dir_all(mount)?;
    fs::create_dir_all(config_dir)?;
    fs::create_dir_all(&runroot)?;

    let disk = crate::guest_init::root::nix::find_btrfs_disk(
        &env_contract.containers_disk_label,
        &env_contract.containers_disk_id,
    )
    .with_context(|| {
        format!(
            "libkrun container storage btrfs disk not found (label={} id={})",
            env_contract.containers_disk_label, env_contract.containers_disk_id
        )
    })?;
    if !command::status_ok("findmnt", &["-rn", path_str(mount)?])? {
        command::run(
            "mount",
            &["-t", "btrfs", path_str(&disk)?, path_str(mount)?],
        )
        .context("failed to mount libkrun container storage btrfs disk")?;
    }
    if let Err(err) = command::run("btrfs", &["filesystem", "resize", "max", path_str(mount)?]) {
        eprintln!(
            "agentbox-guest-init: warning: btrfs resize max failed for '{}': {err:#}; continuing with existing container storage filesystem size",
            mount.display()
        );
    }

    for path in [
        mount,
        storage.as_path(),
        config_dir,
        run_dir.as_path(),
        runroot.as_path(),
    ] {
        fs::create_dir_all(path)?;
        fs::chown(path, identity.uid, identity.gid)?;
    }
    fs::chmod(&run_dir, 0o700)?;
    fs::write_file(
        &config_dir.join("storage.conf"),
        &storage_conf(identity),
        0o644,
    )?;
    fs::write_file(
        &config_dir.join("containers.conf"),
        &containers_conf(tool_paths),
        0o644,
    )?;
    fs::write_file(
        &config_dir.join("registries.conf"),
        registries_conf(),
        0o644,
    )?;
    fs::write_file(&config_dir.join("policy.json"), policy_json(), 0o644)?;
    for file in [
        "storage.conf",
        "containers.conf",
        "registries.conf",
        "policy.json",
    ] {
        fs::chown(&config_dir.join(file), identity.uid, identity.gid)?;
    }
    Ok(())
}

fn append_log(message: &str) -> Result<()> {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(PODMAN_LOG_PATH)?;
    writeln!(file, "{message}")?;
    Ok(())
}

fn path_str(path: &Path) -> Result<&str> {
    path.to_str()
        .ok_or_else(|| anyhow!("path is not valid UTF-8: {}", path.display()))
}

#[cfg(test)]
#[path = "podman_tests.rs"]
mod tests;
