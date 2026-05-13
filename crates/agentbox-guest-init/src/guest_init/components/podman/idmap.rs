use anyhow::{anyhow, bail, Context, Result};
use std::path::Path;

use crate::guest_init::command;
use crate::guest_init::components::home::identity::DevIdentity;
use crate::guest_init::fs;

const SUBID_START: u32 = 100_000;
const SUBID_COUNT: u32 = 65_536;

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

fn install_helper(name: &str) -> Result<()> {
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

fn path_str(path: &Path) -> Result<&str> {
    path.to_str()
        .ok_or_else(|| anyhow!("path is not valid UTF-8: {}", path.display()))
}
