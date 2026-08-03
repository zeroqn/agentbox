use anyhow::{Context, Result, anyhow};
use std::ffi::CString;
use std::io;

use crate::guest_init::components::env::{GuestPermission, GuestPermissions};
use crate::guest_init::components::home::identity::DevIdentity;

const CAP_NET_ADMIN: u32 = 12;
const CAP_NET_RAW: u32 = 13;
const CAP_BPF: u32 = 39;
#[cfg(test)]
const CAP_SYS_ADMIN: u32 = 21;
const CAP_SETUID: u32 = 7;
const CAP_SETGID: u32 = 6;
const CAP_DAC_OVERRIDE: u32 = 1;
const ROOTLESS_IDMAP_CAPABILITIES: [u32; 3] = [CAP_SETUID, CAP_SETGID, CAP_DAC_OVERRIDE];

const VIDEO_GID: libc::gid_t = 44;
const RENDER_GID: libc::gid_t = 107;
const DEV_SUPPLEMENTARY_GROUPS: &[libc::gid_t] = &[VIDEO_GID, RENDER_GID];

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(in crate::guest_init) struct WorkloadCapabilities {
    values: [u32; 3],
    len: usize,
}

impl WorkloadCapabilities {
    pub(in crate::guest_init) fn as_slice(&self) -> &[u32] {
        &self.values[..self.len]
    }

    fn contains(&self, capability: u32) -> bool {
        self.as_slice().contains(&capability)
    }

    #[cfg(test)]
    fn is_empty(self) -> bool {
        self.len == 0
    }
}

pub(in crate::guest_init) fn workload_capability_plan(
    new_permissions: GuestPermissions,
) -> WorkloadCapabilities {
    let mut capabilities = WorkloadCapabilities::default();
    if new_permissions.contains(GuestPermission::NetAdmin) {
        capabilities.values[capabilities.len] = CAP_NET_ADMIN;
        capabilities.len += 1;
    }
    if new_permissions.contains(GuestPermission::NetRaw) {
        capabilities.values[capabilities.len] = CAP_NET_RAW;
        capabilities.len += 1;
    }
    if new_permissions.contains(GuestPermission::Bpf) {
        capabilities.values[capabilities.len] = CAP_BPF;
        capabilities.len += 1;
    }
    capabilities
}

fn retains_bounding_capability(capability: u32, new_capabilities: WorkloadCapabilities) -> bool {
    ROOTLESS_IDMAP_CAPABILITIES.contains(&capability) || new_capabilities.contains(capability)
}

fn restrict_capability_bounding_set(new_capabilities: WorkloadCapabilities) -> io::Result<()> {
    const CAP_SETPCAP: u32 = 8;
    for capability in (0..=40).filter(|capability| *capability != CAP_SETPCAP) {
        if retains_bounding_capability(capability, new_capabilities) {
            continue;
        }
        if unsafe { libc::prctl(libc::PR_CAPBSET_DROP, capability, 0, 0, 0) } != 0 {
            return Err(io::Error::last_os_error());
        }
    }
    if !retains_bounding_capability(CAP_SETPCAP, new_capabilities)
        && unsafe { libc::prctl(libc::PR_CAPBSET_DROP, CAP_SETPCAP, 0, 0, 0) } != 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::guest_init) enum CredentialOperation {
    SupplementaryGroups(&'static [libc::gid_t]),
    PrimaryGid(libc::gid_t),
    Uid(libc::uid_t),
}

impl CredentialOperation {
    fn error_context(self) -> String {
        match self {
            Self::SupplementaryGroups(_) => "failed to set dev supplementary groups".to_owned(),
            Self::PrimaryGid(gid) => format!("failed to set gid {gid}"),
            Self::Uid(uid) => format!("failed to set uid {uid}"),
        }
    }
}

pub(in crate::guest_init) fn credential_plan(identity: &DevIdentity) -> [CredentialOperation; 3] {
    [
        CredentialOperation::SupplementaryGroups(DEV_SUPPLEMENTARY_GROUPS),
        CredentialOperation::PrimaryGid(identity.gid),
        CredentialOperation::Uid(identity.uid),
    ]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CredentialTransitionOperation {
    RestrictCapabilityBoundingSet(WorkloadCapabilities),
    Credential(CredentialOperation),
}

fn credential_transition_plan(
    identity: &DevIdentity,
    new_permissions: GuestPermissions,
) -> Vec<CredentialTransitionOperation> {
    let capabilities = workload_capability_plan(new_permissions);
    let mut operations =
        vec![CredentialTransitionOperation::RestrictCapabilityBoundingSet(capabilities)];
    operations.extend(
        credential_plan(identity)
            .into_iter()
            .map(CredentialTransitionOperation::Credential),
    );
    operations
}

pub(in crate::guest_init) fn apply_dev_credentials(
    identity: &DevIdentity,
    new_permissions: GuestPermissions,
) -> io::Result<()> {
    for operation in credential_transition_plan(identity, new_permissions) {
        let result = match operation {
            CredentialTransitionOperation::RestrictCapabilityBoundingSet(capabilities) => {
                restrict_capability_bounding_set(capabilities)
            }
            CredentialTransitionOperation::Credential(credential) => {
                let rc = match credential {
                    CredentialOperation::SupplementaryGroups(groups) => unsafe {
                        libc::setgroups(groups.len(), groups.as_ptr())
                    },
                    CredentialOperation::PrimaryGid(gid) => unsafe { libc::setgid(gid) },
                    CredentialOperation::Uid(uid) => unsafe { libc::setuid(uid) },
                };
                if rc == 0 {
                    Ok(())
                } else {
                    Err(io::Error::new(
                        io::Error::last_os_error().kind(),
                        format!(
                            "{}: {}",
                            credential.error_context(),
                            io::Error::last_os_error()
                        ),
                    ))
                }
            }
        };
        result?;
    }
    Ok(())
}

pub(in crate::guest_init) fn uid() -> u32 {
    unsafe { libc::getuid() }
}

pub(in crate::guest_init) fn gid() -> u32 {
    unsafe { libc::getgid() }
}

pub(in crate::guest_init) fn is_root() -> bool {
    uid() == 0
}

pub(in crate::guest_init) fn exec_command(command: &[String]) -> Result<()> {
    if command.is_empty() {
        return Err(anyhow!("cannot exec an empty command"));
    }
    execvp(command)
}

pub(in crate::guest_init) fn drop_to_identity_and_exec(
    identity: &DevIdentity,
    permissions: GuestPermissions,
    command: &[String],
) -> Result<()> {
    if command.is_empty() {
        return Err(anyhow!("cannot exec an empty command"));
    }

    apply_dev_credentials(identity, permissions)?;

    execvp(command)
}

pub(in crate::guest_init) fn pid_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    let rc = unsafe { libc::kill(pid as libc::pid_t, 0) };
    rc == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

const GUEST_NOFILE_FLOOR: libc::rlim_t = 524_288;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct NofileLimits {
    soft: libc::rlim_t,
    hard: libc::rlim_t,
}

trait NofileRlimitBackend {
    fn get_nofile_limits(&mut self) -> io::Result<NofileLimits>;
    fn set_nofile_limits(&mut self, limits: NofileLimits) -> io::Result<()>;
}

#[derive(Debug, Default)]
struct LibcNofileRlimitBackend;

impl NofileRlimitBackend for LibcNofileRlimitBackend {
    fn get_nofile_limits(&mut self) -> io::Result<NofileLimits> {
        let mut limits = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        let rc = unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut limits) };
        if rc == 0 {
            Ok(NofileLimits {
                soft: limits.rlim_cur,
                hard: limits.rlim_max,
            })
        } else {
            Err(io::Error::last_os_error())
        }
    }

    fn set_nofile_limits(&mut self, limits: NofileLimits) -> io::Result<()> {
        let raw_limits = libc::rlimit {
            rlim_cur: limits.soft,
            rlim_max: limits.hard,
        };
        let rc = unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &raw_limits) };
        if rc == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }
}

pub(in crate::guest_init) fn ensure_nofile_floor() -> Result<()> {
    let mut backend = LibcNofileRlimitBackend;
    ensure_nofile_floor_with(&mut backend)
}

fn ensure_nofile_floor_with(backend: &mut impl NofileRlimitBackend) -> Result<()> {
    let current = backend
        .get_nofile_limits()
        .context("failed to read guest RLIMIT_NOFILE before launching the guest shell")?;
    let requested = plan_nofile_floor(current)?;
    if requested == current {
        return Ok(());
    }
    backend
        .set_nofile_limits(requested)
        .with_context(|| {
            format!(
                "failed to raise guest RLIMIT_NOFILE from soft={} hard={} to soft={} hard={} before launching the guest shell",
                current.soft, current.hard, requested.soft, requested.hard
            )
        })?;
    Ok(())
}

fn plan_nofile_floor(current: NofileLimits) -> Result<NofileLimits> {
    if current.soft > current.hard {
        return Err(anyhow!(
            "guest RLIMIT_NOFILE is invalid: soft limit {} is greater than hard limit {}",
            current.soft,
            current.hard
        ));
    }
    Ok(NofileLimits {
        soft: current.soft.max(GUEST_NOFILE_FLOOR),
        hard: current.hard.max(GUEST_NOFILE_FLOOR),
    })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct FakeNofileBackend {
        current: io::Result<NofileLimits>,
        set_error: Option<i32>,
        set_calls: Vec<NofileLimits>,
    }

    impl FakeNofileBackend {
        fn with_limits(soft: libc::rlim_t, hard: libc::rlim_t) -> Self {
            Self {
                current: Ok(NofileLimits { soft, hard }),
                set_error: None,
                set_calls: Vec::new(),
            }
        }

        fn with_get_error(errno: i32) -> Self {
            Self {
                current: Err(io::Error::from_raw_os_error(errno)),
                set_error: None,
                set_calls: Vec::new(),
            }
        }
    }

    impl NofileRlimitBackend for FakeNofileBackend {
        fn get_nofile_limits(&mut self) -> io::Result<NofileLimits> {
            self.current.as_ref().map(|limits| *limits).map_err(|err| {
                io::Error::from_raw_os_error(err.raw_os_error().unwrap_or(libc::EIO))
            })
        }

        fn set_nofile_limits(&mut self, limits: NofileLimits) -> io::Result<()> {
            self.set_calls.push(limits);
            self.set_error
                .map_or(Ok(()), |errno| Err(io::Error::from_raw_os_error(errno)))
        }
    }

    #[test]
    fn selected_workload_capabilities_map_exactly() {
        assert_eq!(
            workload_capability_plan(
                "net-admin,net-raw,bpf"
                    .parse()
                    .expect("permissions should parse"),
            )
            .as_slice(),
            [CAP_NET_ADMIN, CAP_NET_RAW, CAP_BPF]
        );
    }

    #[test]
    fn unselected_workload_capabilities_are_empty() {
        assert!(workload_capability_plan(Default::default()).is_empty());
    }

    #[test]
    fn rootless_idmap_capabilities_are_retained_only_in_the_bounding_set() {
        let new_capabilities = workload_capability_plan(Default::default());

        assert!(retains_bounding_capability(CAP_SETUID, new_capabilities));
        assert!(retains_bounding_capability(CAP_SETGID, new_capabilities));
        assert!(retains_bounding_capability(
            CAP_DAC_OVERRIDE,
            new_capabilities
        ));
        assert!(!new_capabilities.contains(CAP_SETUID));
        assert!(!new_capabilities.contains(CAP_SETGID));
        assert!(!new_capabilities.contains(CAP_DAC_OVERRIDE));
        assert!(!retains_bounding_capability(
            CAP_SYS_ADMIN,
            new_capabilities
        ));
    }

    #[test]
    fn new_permissions_extend_the_rootless_idmap_bounding_set() {
        let new_permissions = "net-raw".parse().expect("net-raw permission should parse");
        let new_capabilities = workload_capability_plan(new_permissions);

        assert!(retains_bounding_capability(CAP_SETUID, new_capabilities));
        assert!(retains_bounding_capability(CAP_SETGID, new_capabilities));
        assert!(retains_bounding_capability(
            CAP_DAC_OVERRIDE,
            new_capabilities
        ));
        assert!(retains_bounding_capability(CAP_NET_RAW, new_capabilities));
        assert!(new_capabilities.contains(CAP_NET_RAW));
    }

    #[test]
    fn dev_supplementary_groups_include_wayland_device_groups() {
        assert_eq!(DEV_SUPPLEMENTARY_GROUPS, &[VIDEO_GID, RENDER_GID]);
    }

    #[test]
    fn credential_transition_keeps_new_permissions_out_of_active_workload_sets() {
        let identity = DevIdentity::new(1000, 1000, "/bin/sh".into());
        let permissions = "net-raw".parse().expect("net-raw permission should parse");
        let capabilities = workload_capability_plan(permissions);

        assert_eq!(
            credential_transition_plan(&identity, permissions),
            [
                CredentialTransitionOperation::RestrictCapabilityBoundingSet(capabilities),
                CredentialTransitionOperation::Credential(
                    CredentialOperation::SupplementaryGroups(DEV_SUPPLEMENTARY_GROUPS),
                ),
                CredentialTransitionOperation::Credential(CredentialOperation::PrimaryGid(1000)),
                CredentialTransitionOperation::Credential(CredentialOperation::Uid(1000)),
            ]
        );
    }

    #[test]
    fn dev_credential_plan_preserves_privilege_drop_order() {
        let identity = DevIdentity::new(1000, 1000, "/bin/sh".into());

        assert_eq!(
            credential_plan(&identity),
            [
                CredentialOperation::SupplementaryGroups(DEV_SUPPLEMENTARY_GROUPS),
                CredentialOperation::PrimaryGid(identity.gid),
                CredentialOperation::Uid(identity.uid),
            ]
        );
    }

    #[test]
    fn dev_credential_operations_preserve_syscall_error_context() {
        let identity = DevIdentity::new(1000, 1000, "/bin/sh".into());

        assert_eq!(
            credential_plan(&identity).map(CredentialOperation::error_context),
            [
                "failed to set dev supplementary groups".to_owned(),
                "failed to set gid 1000".to_owned(),
                "failed to set uid 1000".to_owned(),
            ]
        );
    }

    #[test]
    fn nofile_floor_preserves_higher_limits_without_setrlimit() {
        let mut backend = FakeNofileBackend::with_limits(600_000, 700_000);

        ensure_nofile_floor_with(&mut backend).expect("limits above floor should pass");

        assert!(backend.set_calls.is_empty());
    }

    #[test]
    fn nofile_floor_raises_both_soft_and_hard_when_below_floor() {
        let mut backend = FakeNofileBackend::with_limits(1024, 4096);

        ensure_nofile_floor_with(&mut backend).expect("below-floor limits should be raised");

        assert_eq!(
            backend.set_calls,
            [NofileLimits {
                soft: GUEST_NOFILE_FLOOR,
                hard: GUEST_NOFILE_FLOOR,
            }]
        );
    }

    #[test]
    fn nofile_floor_raises_only_soft_when_hard_is_already_high_enough() {
        let mut backend = FakeNofileBackend::with_limits(1024, 700_000);

        ensure_nofile_floor_with(&mut backend).expect("soft limit should be raised to floor");

        assert_eq!(
            backend.set_calls,
            [NofileLimits {
                soft: GUEST_NOFILE_FLOOR,
                hard: 700_000,
            }]
        );
    }

    #[test]
    fn nofile_floor_rejects_invalid_limits() {
        let err = plan_nofile_floor(NofileLimits {
            soft: 4096,
            hard: 1024,
        })
        .expect_err("soft above hard should fail");

        assert!(
            format!("{err:#}").contains("guest RLIMIT_NOFILE is invalid"),
            "unexpected error: {err:#}"
        );
    }

    #[test]
    fn nofile_floor_reports_getrlimit_failure() {
        let mut backend = FakeNofileBackend::with_get_error(libc::EPERM);

        let err =
            ensure_nofile_floor_with(&mut backend).expect_err("getrlimit failure should surface");

        assert!(format!("{err:#}").contains("failed to read guest RLIMIT_NOFILE"));
    }

    #[test]
    fn nofile_floor_reports_setrlimit_failure() {
        let mut backend = FakeNofileBackend::with_limits(1024, 4096);
        backend.set_error = Some(libc::EPERM);

        let err =
            ensure_nofile_floor_with(&mut backend).expect_err("setrlimit failure should surface");

        assert!(format!("{err:#}").contains("failed to raise guest RLIMIT_NOFILE"));
    }
}
