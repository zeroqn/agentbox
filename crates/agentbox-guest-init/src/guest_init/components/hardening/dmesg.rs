use anyhow::{Context, Result};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;

const DMESG_RESTRICT_PATH: &str = "/proc/sys/kernel/dmesg_restrict";

/// Owns the guest dmesg restriction sysctl for libkrun hardening.
pub(in crate::guest_init) fn restrict() -> Result<()> {
    restrict_at(Path::new(DMESG_RESTRICT_PATH))
}

fn restrict_at(path: &Path) -> Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(path)
        .with_context(|| format!("failed to open {} for dmesg restriction", path.display()))?;
    file.write_all(b"1\n")
        .with_context(|| format!("failed to set {}=1 for dmesg restriction", path.display()))
}

#[cfg(test)]
#[path = "dmesg_tests.rs"]
mod tests;
