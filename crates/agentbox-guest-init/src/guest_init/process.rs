use anyhow::{Context, Result, anyhow};
use std::ffi::CString;

use crate::guest_init::components::home::identity::DevIdentity;

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

pub(in crate::guest_init) fn pid_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    let rc = unsafe { libc::kill(pid as libc::pid_t, 0) };
    rc == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
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
