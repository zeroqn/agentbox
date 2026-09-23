use anyhow::Result;
use std::path::PathBuf;

use crate::guest_init::components::home::identity::DevIdentity;
use crate::guest_init::fs;

pub(in crate::guest_init) fn ensure_user_runtime_dir(identity: &DevIdentity) -> Result<PathBuf> {
    let run_dir = PathBuf::from(format!("/run/user/{}", identity.uid));
    fs::create_dir_all(&run_dir)?;
    fs::chown(&run_dir, identity.uid, identity.gid)?;
    fs::chmod(&run_dir, 0o700)?;
    Ok(run_dir)
}

#[cfg(test)]
#[path = "runtime_dir_tests.rs"]
mod tests;
