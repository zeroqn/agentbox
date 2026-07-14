use anyhow::{Context, Result, anyhow};
use std::ffi::CString;
use std::io;

use crate::guest_init::components::home::identity::DevIdentity;

const VIDEO_GID: libc::gid_t = 44;
const RENDER_GID: libc::gid_t = 107;
const DEV_SUPPLEMENTARY_GROUPS: &[libc::gid_t] = &[VIDEO_GID, RENDER_GID];

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

pub(in crate::guest_init) fn apply_dev_credentials(identity: &DevIdentity) -> io::Result<()> {
    for operation in credential_plan(identity) {
        let rc = match operation {
            CredentialOperation::SupplementaryGroups(groups) => unsafe {
                libc::setgroups(groups.len(), groups.as_ptr())
            },
            CredentialOperation::PrimaryGid(gid) => unsafe { libc::setgid(gid) },
            CredentialOperation::Uid(uid) => unsafe { libc::setuid(uid) },
        };
        if rc != 0 {
            let error = io::Error::last_os_error();
            return Err(io::Error::new(
                error.kind(),
                format!("{}: {error}", operation.error_context()),
            ));
        }
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
    command: &[String],
) -> Result<()> {
    if command.is_empty() {
        return Err(anyhow!("cannot exec an empty command"));
    }

    apply_dev_credentials(identity)?;

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
    fn dev_supplementary_groups_include_wayland_device_groups() {
        assert_eq!(DEV_SUPPLEMENTARY_GROUPS, &[VIDEO_GID, RENDER_GID]);
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
