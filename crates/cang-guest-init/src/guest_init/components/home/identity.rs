use anyhow::{Result, bail};
use std::path::PathBuf;

use crate::guest_init::components::env::DEV_HOME;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::guest_init) struct DevIdentity {
    pub(in crate::guest_init) uid: u32,
    pub(in crate::guest_init) gid: u32,
    pub(in crate::guest_init) home: PathBuf,
    pub(in crate::guest_init) shell: PathBuf,
}

impl DevIdentity {
    pub(in crate::guest_init) fn new(uid: u32, gid: u32, shell: PathBuf) -> Self {
        Self {
            uid,
            gid,
            home: PathBuf::from(DEV_HOME),
            shell,
        }
    }
}

pub(in crate::guest_init) fn validate_host_identity(uid: u32, gid: u32) -> Result<()> {
    if uid == 0 || gid == 0 {
        bail!("internal host UID/GID must identify the non-root dev user, got {uid}:{gid}");
    }
    Ok(())
}

#[cfg(test)]
#[path = "identity_tests.rs"]
mod tests;
