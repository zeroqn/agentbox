use anyhow::{anyhow, bail, Context, Result};
use std::path::{Path, PathBuf};

use crate::guest_init::command;
use crate::guest_init::components::home::identity::DevIdentity;
use crate::guest_init::fs;

const SUBID_START: u32 = 100_000;
const SUBID_COUNT: u32 = 65_536;
pub(in crate::guest_init) const HELPER_DIR: &str = "/run/agentbox/idmap-bin";

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
    let helper_dir = Path::new(HELPER_DIR);
    fs::create_dir_all(helper_dir)?;
    let dst = helper_dir.join(name);
    let src = source_helper_on_path(name, helper_dir)?;
    if src == dst {
        return Ok(());
    }
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

fn source_helper_on_path(name: &str, helper_dir: &Path) -> Result<PathBuf> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path)
        .filter(|dir| dir != helper_dir)
        .map(|dir| dir.join(name))
        .find(|candidate| command::is_executable(candidate))
        .or_else(|| {
            let existing = helper_dir.join(name);
            command::is_executable(&existing).then_some(existing)
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
