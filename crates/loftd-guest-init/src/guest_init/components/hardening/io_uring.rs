use anyhow::{Context, Result};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;

const IO_URING_DISABLED_PATH: &str = "/proc/sys/kernel/io_uring_disabled";
const IO_URING_GROUP_PATH: &str = "/proc/sys/kernel/io_uring_group";

pub(in crate::guest_init) fn configure(enabled: bool, dev_gid: u32) -> Result<()> {
    configure_at(
        Path::new(IO_URING_DISABLED_PATH),
        Path::new(IO_URING_GROUP_PATH),
        enabled,
        dev_gid,
    )
}

fn configure_at(
    disabled_path: &Path,
    group_path: &Path,
    enabled: bool,
    dev_gid: u32,
) -> Result<()> {
    if enabled {
        write_sysctl(group_path, "io_uring_group", &format!("{dev_gid}\n"))
    } else {
        write_sysctl(disabled_path, "io_uring_disabled", "2\n")
    }
}

fn write_sysctl(path: &Path, name: &str, value: &str) -> Result<()> {
    let setting = value.trim_end();
    let mut file = OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(path)
        .with_context(|| format!("failed to open {} to set {name}={setting}", path.display()))?;
    file.write_all(value.as_bytes())
        .with_context(|| format!("failed to set {} {name}={setting}", path.display()))
}

#[cfg(test)]
#[path = "io_uring_tests.rs"]
mod tests;
