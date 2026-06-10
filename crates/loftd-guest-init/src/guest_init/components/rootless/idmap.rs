use anyhow::{Context, Result, anyhow, bail};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::guest_init::command;
use crate::guest_init::components::home::identity::DevIdentity;
use crate::guest_init::fs;

const SUBID_START: u32 = 100_000;
const SUBID_COUNT: u32 = 65_536;
pub(in crate::guest_init) const WRAPPER_BIN_DIR: &str = "/run/loftd/wrappers/bin";

pub(in crate::guest_init) fn prepare(identity: &DevIdentity) -> Result<()> {
    materialize_subid_files(identity)?;
    install_helper("newuidmap")?;
    install_helper("newgidmap")
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
                "failed to materialize {} for rootless container runtimes",
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

fn install_helper(name: &str) -> Result<()> {
    let wrapper_dir = Path::new(WRAPPER_BIN_DIR);
    fs::create_dir_all(wrapper_dir)?;
    let dst = wrapper_dir.join(name);
    let src = source_helper_on_path(name, wrapper_dir)?;
    if src == dst || installed_helper_is_ready(&dst) {
        return Ok(());
    }
    let install_result = command::run(
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
    );
    match install_result {
        Ok(()) => Ok(()),
        Err(err) => {
            if wait_for_installed_helper_ready(&dst) {
                Ok(())
            } else {
                Err(err)
                    .with_context(|| format!("failed to install root-owned setuid {name} helper"))
            }
        }
    }
}

fn wait_for_installed_helper_ready(path: &Path) -> bool {
    let deadline = Instant::now() + Duration::from_secs(1);
    loop {
        if installed_helper_is_ready(path) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn installed_helper_is_ready(path: &Path) -> bool {
    path.metadata()
        .map(|metadata| {
            helper_metadata_is_ready(
                metadata.is_file(),
                metadata.permissions().mode(),
                metadata.uid(),
                metadata.gid(),
            )
        })
        .unwrap_or(false)
}

fn helper_metadata_is_ready(is_file: bool, mode: u32, uid: u32, gid: u32) -> bool {
    is_file && uid == 0 && gid == 0 && mode & 0o111 != 0 && mode & 0o4000 != 0
}

fn source_helper_on_path(name: &str, wrapper_dir: &Path) -> Result<PathBuf> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    source_helper_in_path(name, wrapper_dir, &path)
}

fn source_helper_in_path(
    name: &str,
    wrapper_dir: &Path,
    path: &std::ffi::OsStr,
) -> Result<PathBuf> {
    std::env::split_paths(path)
        .filter(|dir| dir != wrapper_dir)
        .map(|dir| dir.join(name))
        .find(|candidate| command::is_executable(candidate))
        .or_else(|| {
            let existing = wrapper_dir.join(name);
            installed_helper_is_ready(&existing).then_some(existing)
        })
        .ok_or_else(|| anyhow!("required tool '{name}' is not available on PATH"))
}

fn path_str(path: &Path) -> Result<&str> {
    path.to_str()
        .ok_or_else(|| anyhow!("path is not valid UTF-8: {}", path.display()))
}

#[cfg(test)]
#[path = "idmap_tests.rs"]
mod tests;
