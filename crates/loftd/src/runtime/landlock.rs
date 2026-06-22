//! Host-side Landlock confinement for the loftd VM worker.
//!
//! This module owns the kernel-independent effective-policy model and the thin
//! adapter that translates that model to the rust-landlock crate immediately
//! before `krun_start_enter`.

use anyhow::{Context, Result, bail};
use clap::ValueEnum;
use landlock::{
    ABI, Access, AccessFs, AccessNet, CompatLevel, Compatible, NetPort, PathBeneath, PathFd,
    RestrictSelfAttr, RestrictionStatus, Ruleset, RulesetAttr, RulesetCreatedAttr, RulesetStatus,
    Scope, make_bitflags,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::runtime::launch::config::{BindMount, DiskAttachment, HostNixOverlay, LaunchConfig};

const FD_DIR: &str = "/proc/self/fd";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, ValueEnum, Serialize, Deserialize)]
pub(crate) enum LandlockMode {
    #[default]
    Enforce,
    BestEffort,
    Off,
}

impl LandlockMode {
    pub(crate) fn as_config_value(self) -> &'static str {
        match self {
            Self::Enforce => "enforce",
            Self::BestEffort => "best-effort",
            Self::Off => "off",
        }
    }

    pub(crate) fn parse_config_value(mode: Option<&str>) -> Result<Self> {
        match mode.unwrap_or("enforce") {
            "enforce" => Ok(Self::Enforce),
            "best-effort" => Ok(Self::BestEffort),
            "off" => Ok(Self::Off),
            _ => bail!("loftd launch config landlock.mode is invalid"),
        }
    }

    fn compatibility(self) -> CompatLevel {
        match self {
            Self::Enforce => CompatLevel::HardRequirement,
            Self::BestEffort | Self::Off => CompatLevel::BestEffort,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EffectivePolicy {
    pub(crate) mode: LandlockMode,
    pub(crate) path_rules: Vec<PathRule>,
    pub(crate) bind_tcp_ports: Vec<u16>,
    pub(crate) connect_tcp: ConnectTcpPolicy,
    pub(crate) ipc_scopes: Vec<IpcScope>,
    pub(crate) audit: AuditPolicy,
    pub(crate) fd_report: RetainedFdReport,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PathAccess {
    ReadOnly,
    ReadWrite,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PathRule {
    pub(crate) category: PathCategory,
    pub(crate) path: PathBuf,
    pub(crate) access: PathAccess,
    pub(crate) read_only_guarantee: ReadOnlyGuarantee,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PathCategory {
    PreparedRoot,
    BindMount { target: String },
    Disk { id: String },
    GuestInitOverride,
    HostNixOverlay { role: HostNixOverlayRole },
    ProfileOutput,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HostNixOverlayRole {
    Lower,
    Upper,
    Work,
    Merged,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReadOnlyGuarantee {
    LandlockEnforced,
    MountEnforced,
    NotReadOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConnectTcpPolicy {
    UnrestrictedByDesign,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IpcScope {
    AbstractUnixSocket,
    Signal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AuditPolicy {
    pub(crate) log_same_exec: bool,
    pub(crate) log_new_exec: bool,
    pub(crate) log_subdomains: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct RetainedFdReport {
    pub(crate) entries: Vec<RetainedFd>,
    pub(crate) unexpected: Vec<RetainedFd>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RetainedFd {
    pub(crate) fd: i32,
    pub(crate) target: String,
    pub(crate) classification: RetainedFdClass,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RetainedFdClass {
    Stdio,
    PasstSocket,
    RuntimeKernelObject,
    BenignDevice,
    Unexpected,
}

pub(crate) fn apply(
    config: &LaunchConfig,
    task_state_dir: &Path,
    profile_enabled: bool,
) -> Result<()> {
    if config.landlock == LandlockMode::Off {
        tracing::debug!(landlock = "off", "loftd internal: Landlock disabled");
        return Ok(());
    }

    let policy = EffectivePolicy::build(config, task_state_dir, profile_enabled)?;
    tracing::debug!(report = %policy.report(), "loftd internal: Landlock effective policy");

    apply_policy(&policy).with_context(|| {
        format!(
            "failed to apply loftd Landlock policy in {} mode",
            policy.mode.as_config_value()
        )
    })?;
    Ok(())
}

impl EffectivePolicy {
    pub(crate) fn build(
        config: &LaunchConfig,
        task_state_dir: &Path,
        profile_enabled: bool,
    ) -> Result<Self> {
        let fd_report = retained_fd_report(config.landlock, config.passt_fd)?;
        Self::build_with_fd_report(config, task_state_dir, profile_enabled, fd_report)
    }

    fn build_with_fd_report(
        config: &LaunchConfig,
        task_state_dir: &Path,
        profile_enabled: bool,
        fd_report: RetainedFdReport,
    ) -> Result<Self> {
        let mut path_rules = Vec::new();
        path_rules.push(PathRule::new(
            PathCategory::PreparedRoot,
            config.task_rootfs.clone(),
            PathAccess::ReadOnly,
        ));
        for mount in &config.mounts {
            path_rules.push(bind_mount_rule(mount));
        }
        if let Some(mount) = &config.guest_init_override {
            path_rules.push(PathRule::new(
                PathCategory::GuestInitOverride,
                mount.source.clone(),
                if mount.read_only {
                    PathAccess::ReadOnly
                } else {
                    PathAccess::ReadWrite
                },
            ));
        }
        for disk in &config.disks {
            path_rules.push(disk_rule(disk));
        }
        if let Some(overlay) = &config.host_nix_overlay {
            path_rules.extend(host_nix_overlay_rules(overlay));
        }
        if profile_enabled {
            path_rules.push(PathRule::new(
                PathCategory::ProfileOutput,
                task_state_dir.to_path_buf(),
                PathAccess::ReadWrite,
            ));
        }

        normalize_path_rules(&mut path_rules);
        compute_read_only_guarantees(&mut path_rules);

        let bind_tcp_ports = bind_tcp_ports(config)?;
        Ok(Self {
            mode: config.landlock,
            path_rules,
            bind_tcp_ports,
            connect_tcp: ConnectTcpPolicy::UnrestrictedByDesign,
            ipc_scopes: vec![IpcScope::AbstractUnixSocket, IpcScope::Signal],
            audit: AuditPolicy {
                log_same_exec: false,
                log_new_exec: true,
                log_subdomains: false,
            },
            fd_report,
        })
    }

    pub(crate) fn report(&self) -> String {
        let path_summary = self
            .path_rules
            .iter()
            .map(|rule| {
                format!(
                    "{}:{}:{:?}:{:?}",
                    rule.category.as_report_label(),
                    rule.path.display(),
                    rule.access,
                    rule.read_only_guarantee
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        let ports = self
            .bind_tcp_ports
            .iter()
            .map(u16::to_string)
            .collect::<Vec<_>>()
            .join(",");
        let fds = self
            .fd_report
            .entries
            .iter()
            .chain(self.fd_report.unexpected.iter())
            .map(|entry| format!("{}:{}:{:?}", entry.fd, entry.target, entry.classification))
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "mode={} fs_rules={} bind_tcp=[{}] connect_tcp={:?} ipc_scopes={:?} audit={{same_exec:{},new_exec:{},subdomains:{}}} retained_fds=[{}]",
            self.mode.as_config_value(),
            path_summary,
            ports,
            self.connect_tcp,
            self.ipc_scopes,
            self.audit.log_same_exec,
            self.audit.log_new_exec,
            self.audit.log_subdomains,
            fds,
        )
    }
}

impl PathRule {
    fn new(category: PathCategory, path: PathBuf, access: PathAccess) -> Self {
        Self {
            category,
            path,
            access,
            read_only_guarantee: match access {
                PathAccess::ReadOnly => ReadOnlyGuarantee::LandlockEnforced,
                PathAccess::ReadWrite => ReadOnlyGuarantee::NotReadOnly,
            },
        }
    }
}

impl PathCategory {
    fn as_report_label(&self) -> String {
        match self {
            Self::PreparedRoot => "prepared-root".to_owned(),
            Self::BindMount { target } => format!("bind:{target}"),
            Self::Disk { id } => format!("disk:{id}"),
            Self::GuestInitOverride => "guest-init-override".to_owned(),
            Self::HostNixOverlay { role } => format!("host-nix-overlay:{role:?}"),
            Self::ProfileOutput => "profile-output".to_owned(),
        }
    }
}

fn bind_mount_rule(mount: &BindMount) -> PathRule {
    PathRule::new(
        PathCategory::BindMount {
            target: mount.target.clone(),
        },
        mount.source.clone(),
        if mount.read_only {
            PathAccess::ReadOnly
        } else {
            PathAccess::ReadWrite
        },
    )
}

fn disk_rule(disk: &DiskAttachment) -> PathRule {
    PathRule::new(
        PathCategory::Disk {
            id: disk.id.clone(),
        },
        disk.path.clone(),
        if disk.read_only {
            PathAccess::ReadOnly
        } else {
            PathAccess::ReadWrite
        },
    )
}

fn host_nix_overlay_rules(overlay: &HostNixOverlay) -> Vec<PathRule> {
    vec![
        PathRule::new(
            PathCategory::HostNixOverlay {
                role: HostNixOverlayRole::Lower,
            },
            overlay.lowerdir.clone(),
            PathAccess::ReadOnly,
        ),
        PathRule::new(
            PathCategory::HostNixOverlay {
                role: HostNixOverlayRole::Upper,
            },
            overlay.upperdir.clone(),
            PathAccess::ReadWrite,
        ),
        PathRule::new(
            PathCategory::HostNixOverlay {
                role: HostNixOverlayRole::Work,
            },
            overlay.workdir.clone(),
            PathAccess::ReadWrite,
        ),
        PathRule::new(
            PathCategory::HostNixOverlay {
                role: HostNixOverlayRole::Merged,
            },
            overlay.mergeddir.clone(),
            PathAccess::ReadOnly,
        ),
    ]
}

fn normalize_path_rules(rules: &mut Vec<PathRule>) {
    rules.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| access_rank(right.access).cmp(&access_rank(left.access)))
            .then_with(|| {
                left.category
                    .as_report_label()
                    .cmp(&right.category.as_report_label())
            })
    });
    rules.dedup_by(|left, right| {
        left.path == right.path && left.access == right.access && left.category == right.category
    });
}

fn access_rank(access: PathAccess) -> u8 {
    match access {
        PathAccess::ReadOnly => 0,
        PathAccess::ReadWrite => 1,
    }
}

fn compute_read_only_guarantees(rules: &mut [PathRule]) {
    let writable_paths = rules
        .iter()
        .filter(|rule| rule.access == PathAccess::ReadWrite)
        .map(|rule| rule.path.clone())
        .collect::<Vec<_>>();
    for rule in rules {
        rule.read_only_guarantee = match rule.access {
            PathAccess::ReadWrite => ReadOnlyGuarantee::NotReadOnly,
            PathAccess::ReadOnly => {
                if writable_paths
                    .iter()
                    .any(|write_path| write_path != &rule.path && rule.path.starts_with(write_path))
                {
                    ReadOnlyGuarantee::MountEnforced
                } else {
                    ReadOnlyGuarantee::LandlockEnforced
                }
            }
        };
    }
}

fn bind_tcp_ports(config: &LaunchConfig) -> Result<Vec<u16>> {
    let mut ports = BTreeSet::new();
    for spec in &config.publish {
        if let Some(port) = parse_publish_host_port(spec)? {
            ports.insert(port);
        }
    }
    Ok(ports.into_iter().collect())
}

fn parse_publish_host_port(spec: &str) -> Result<Option<u16>> {
    let spec = spec.trim();
    if spec.is_empty() {
        return Ok(None);
    }
    let payload = match spec.split_once(':') {
        Some((selector, payload)) if selector.eq_ignore_ascii_case("tcp") => payload,
        Some((selector, _)) if selector.eq_ignore_ascii_case("udp") => return Ok(None),
        _ => spec,
    };
    let Some((host, _guest)) = payload.split_once(':') else {
        return Ok(None);
    };
    if host.contains(['-', '~', '/', '%', ',']) || host.is_empty() {
        return Ok(None);
    }
    let port = host
        .parse::<u16>()
        .with_context(|| format!("failed to parse TCP publish host port from '{spec}'"))?;
    if port == 0 {
        return Ok(None);
    }
    Ok(Some(port))
}

fn retained_fd_report(mode: LandlockMode, passt_fd: Option<i32>) -> Result<RetainedFdReport> {
    let mut entries = Vec::new();
    let mut unexpected = Vec::new();
    let passt_fd = passt_fd.into_iter().collect::<HashSet<_>>();
    let fd_dir = match fs::read_dir(FD_DIR) {
        Ok(dir) => dir,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(RetainedFdReport::default());
        }
        Err(err) => return Err(err).context("failed to inventory retained file descriptors"),
    };
    for entry in fd_dir {
        let entry = entry.context("failed to read retained file descriptor entry")?;
        let Some(fd) = entry.file_name().to_string_lossy().parse::<i32>().ok() else {
            continue;
        };
        let target = fs::read_link(entry.path())
            .map(|path| path.display().to_string())
            .unwrap_or_else(|_| "<unavailable>".to_owned());
        if let Some(classification) = classify_fd(fd, &target, &passt_fd) {
            entries.push(RetainedFd {
                fd,
                target,
                classification,
            });
        } else {
            let retained = RetainedFd {
                fd,
                target,
                classification: RetainedFdClass::Unexpected,
            };
            if mode == LandlockMode::Enforce {
                bail!(
                    "loftd Landlock strict mode refuses unexpected retained fd {} -> {}; close or categorize the descriptor before krun_start_enter",
                    retained.fd,
                    retained.target,
                );
            }
            unexpected.push(retained);
        }
    }
    entries.sort_by_key(|entry| entry.fd);
    unexpected.sort_by_key(|entry| entry.fd);
    Ok(RetainedFdReport {
        entries,
        unexpected,
    })
}

fn classify_fd(fd: i32, target: &str, passt_fds: &HashSet<i32>) -> Option<RetainedFdClass> {
    if (0..=2).contains(&fd) {
        return Some(RetainedFdClass::Stdio);
    }
    if passt_fds.contains(&fd) {
        return Some(RetainedFdClass::PasstSocket);
    }
    if target.starts_with("anon_inode:")
        || target.starts_with("socket:")
        || target.starts_with("pipe:")
    {
        return Some(RetainedFdClass::RuntimeKernelObject);
    }
    if target.starts_with("/proc/") && target.ends_with("/fd") {
        return Some(RetainedFdClass::RuntimeKernelObject);
    }
    if target == "/dev/null" || target == "/dev/tty" {
        return Some(RetainedFdClass::BenignDevice);
    }
    None
}

fn apply_policy(policy: &EffectivePolicy) -> Result<()> {
    let compat = policy.mode.compatibility();
    let fs_access = AccessFs::from_all(ABI::V5);
    let net_access = make_bitflags!(AccessNet::{BindTcp});
    let scopes = make_bitflags!(Scope::{AbstractUnixSocket | Signal});

    let mut ruleset = Ruleset::default()
        .set_compatibility(compat)
        .handle_access(fs_access)?
        .handle_access(net_access)?
        .scope(scopes)?
        .create()?
        .set_compatibility(compat);

    for rule in &policy.path_rules {
        ruleset = ruleset.add_rule(
            PathBeneath::new(PathFd::new(&rule.path)?, landlock_access_for(rule.access))
                .set_compatibility(compat),
        )?;
    }
    for port in &policy.bind_tcp_ports {
        ruleset =
            ruleset.add_rule(NetPort::new(*port, AccessNet::BindTcp).set_compatibility(compat))?;
    }

    let status = ruleset
        .log_same_exec(policy.audit.log_same_exec)?
        .log_new_exec(policy.audit.log_new_exec)?
        .log_subdomains(policy.audit.log_subdomains)?
        .restrict_self()?;
    ensure_status(policy.mode, status)
}

fn landlock_access_for(access: PathAccess) -> landlock::BitFlags<AccessFs> {
    let read = make_bitflags!(AccessFs::{Execute | ReadFile | ReadDir});
    match access {
        PathAccess::ReadOnly => read,
        PathAccess::ReadWrite => {
            read | make_bitflags!(AccessFs::{
                WriteFile | RemoveDir | RemoveFile | MakeChar | MakeDir | MakeReg | MakeSock |
                MakeFifo | MakeBlock | MakeSym | Refer | Truncate | IoctlDev
            })
        }
    }
}

fn ensure_status(mode: LandlockMode, status: RestrictionStatus) -> Result<()> {
    if mode == LandlockMode::Enforce
        && (status.ruleset != RulesetStatus::FullyEnforced || !status.no_new_privs)
    {
        bail!("loftd Landlock enforce mode was not fully enforced: {status:?}");
    }
    tracing::debug!(?status, "loftd internal: Landlock restriction status");
    Ok(())
}

#[cfg(test)]
pub(crate) fn ensure_fully_enforced_for_test(
    mode: LandlockMode,
    fully_enforced: bool,
) -> Result<()> {
    if mode == LandlockMode::Enforce && !fully_enforced {
        bail!("loftd Landlock enforce mode was not fully enforced");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logging::LogLevel;
    use crate::runtime::launch::config::{BindMountSourceKind, NetworkMode};
    use crate::runtime::seccomp::SeccompMode;

    fn test_config(task_rootfs: &Path) -> LaunchConfig {
        LaunchConfig {
            task_rootfs: task_rootfs.to_path_buf(),
            hostname: "loftd-test".to_owned(),
            mounts: Vec::new(),
            host_nix_overlay: None,
            guest_init_override: None,
            disks: Vec::new(),
            ram_mib: 1024,
            vcpus: 1,
            log_level: LogLevel::Off,
            network_mode: NetworkMode::Tsi,
            publish: Vec::new(),
            workdir: "/workspace".to_owned(),
            exec_path: "/loftd-guest-init".to_owned(),
            argv: Vec::new(),
            env: Vec::new(),
            guest_config_env: Vec::new(),
            passt_fd: None,
            managed_session: None,
            seccomp: SeccompMode::Off,
            landlock: LandlockMode::Enforce,
        }
    }

    #[test]
    fn parses_config_modes_with_enforce_default() {
        assert_eq!(
            LandlockMode::parse_config_value(None).unwrap(),
            LandlockMode::Enforce
        );
        assert_eq!(
            LandlockMode::parse_config_value(Some("enforce")).unwrap(),
            LandlockMode::Enforce
        );
        assert_eq!(
            LandlockMode::parse_config_value(Some("best-effort")).unwrap(),
            LandlockMode::BestEffort
        );
        assert_eq!(
            LandlockMode::parse_config_value(Some("off")).unwrap(),
            LandlockMode::Off
        );
        assert!(LandlockMode::parse_config_value(Some("audit")).is_err());
    }

    #[test]
    fn effective_policy_marks_connect_tcp_unrestricted_by_design() {
        let dir = tempfile::tempdir().unwrap();
        let config = test_config(dir.path());

        let policy = EffectivePolicy::build_with_fd_report(
            &config,
            dir.path(),
            false,
            RetainedFdReport::default(),
        )
        .unwrap();

        assert_eq!(policy.connect_tcp, ConnectTcpPolicy::UnrestrictedByDesign);
    }

    #[test]
    fn effective_policy_preserves_read_only_and_read_write_path_intent() {
        let dir = tempfile::tempdir().unwrap();
        let ro = dir.path().join("ro");
        let rw = dir.path().join("rw");
        fs::create_dir_all(&ro).unwrap();
        fs::create_dir_all(&rw).unwrap();
        let mut config = test_config(dir.path().join("rootfs").as_path());
        config.mounts = vec![
            BindMount {
                source: ro.clone(),
                tag: "ro".to_owned(),
                target: "/ro".to_owned(),
                source_kind: BindMountSourceKind::Directory,
                read_only: true,
            },
            BindMount {
                source: rw.clone(),
                tag: "rw".to_owned(),
                target: "/rw".to_owned(),
                source_kind: BindMountSourceKind::Directory,
                read_only: false,
            },
        ];

        let policy = EffectivePolicy::build_with_fd_report(
            &config,
            dir.path(),
            false,
            RetainedFdReport::default(),
        )
        .unwrap();

        let ro_rule = policy
            .path_rules
            .iter()
            .find(|rule| rule.path == ro)
            .unwrap();
        assert_eq!(ro_rule.access, PathAccess::ReadOnly);
        assert_eq!(
            ro_rule.read_only_guarantee,
            ReadOnlyGuarantee::LandlockEnforced
        );
        let rw_rule = policy
            .path_rules
            .iter()
            .find(|rule| rule.path == rw)
            .unwrap();
        assert_eq!(rw_rule.access, PathAccess::ReadWrite);
        assert_eq!(rw_rule.read_only_guarantee, ReadOnlyGuarantee::NotReadOnly);
    }

    #[test]
    fn read_only_child_reports_mount_enforced_when_parent_is_writable() {
        let dir = tempfile::tempdir().unwrap();
        let child = dir.path().join("child");
        fs::create_dir_all(&child).unwrap();
        let config = test_config(child.as_path());

        let policy = EffectivePolicy::build_with_fd_report(
            &config,
            dir.path(),
            true,
            RetainedFdReport::default(),
        )
        .unwrap();
        let prepared_root = policy
            .path_rules
            .iter()
            .find(|rule| rule.category == PathCategory::PreparedRoot)
            .unwrap();

        assert_eq!(
            prepared_root.read_only_guarantee,
            ReadOnlyGuarantee::MountEnforced
        );
    }

    #[test]
    fn bind_tcp_ports_include_simple_tcp_publish_and_ignore_connect_tcp() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = test_config(dir.path());
        config.publish = vec![
            "8080:80".to_owned(),
            "tcp:8443:443".to_owned(),
            "udp:5353:5353".to_owned(),
            "10000-10010:80-90".to_owned(),
        ];

        let policy = EffectivePolicy::build_with_fd_report(
            &config,
            dir.path(),
            false,
            RetainedFdReport::default(),
        )
        .unwrap();

        assert_eq!(policy.bind_tcp_ports, vec![8080, 8443]);
        assert_eq!(policy.connect_tcp, ConnectTcpPolicy::UnrestrictedByDesign);
    }

    #[test]
    fn strict_status_helper_fails_when_not_fully_enforced() {
        assert!(ensure_fully_enforced_for_test(LandlockMode::Enforce, false).is_err());
        assert!(ensure_fully_enforced_for_test(LandlockMode::BestEffort, false).is_ok());
        assert!(ensure_fully_enforced_for_test(LandlockMode::Enforce, true).is_ok());
    }

    #[test]
    fn fd_classifier_rejects_unexpected_regular_files() {
        assert_eq!(
            classify_fd(0, "/tmp/file", &HashSet::new()),
            Some(RetainedFdClass::Stdio)
        );
        assert_eq!(
            classify_fd(9, "socket:[123]", &HashSet::new()),
            Some(RetainedFdClass::RuntimeKernelObject)
        );
        assert_eq!(classify_fd(9, "/tmp/file", &HashSet::new()), None);
    }

    #[test]
    fn unexpected_fd_policy_is_strict_only() {
        let temp = tempfile::NamedTempFile::new().unwrap();
        let target = temp.path().display().to_string();
        let fd = 99;
        let report = RetainedFdReport {
            entries: Vec::new(),
            unexpected: vec![RetainedFd {
                fd,
                target: target.clone(),
                classification: RetainedFdClass::Unexpected,
            }],
        };
        let dir = tempfile::tempdir().unwrap();
        let mut config = test_config(dir.path());
        config.landlock = LandlockMode::BestEffort;

        let policy = EffectivePolicy::build_with_fd_report(&config, dir.path(), false, report)
            .expect("best-effort should carry unexpected retained fd in report");

        assert_eq!(
            policy.fd_report.unexpected[0].classification,
            RetainedFdClass::Unexpected
        );
        assert_eq!(policy.fd_report.unexpected[0].target, target);
    }
}
