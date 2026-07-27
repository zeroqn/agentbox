use anyhow::{Context, Result};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;

const PERF_EVENT_PARANOID_PATH: &str = "/proc/sys/kernel/perf_event_paranoid";
const KPTR_RESTRICT_PATH: &str = "/proc/sys/kernel/kptr_restrict";

pub(in crate::guest_init) fn configure(enabled: bool) -> Result<()> {
    configure_at(
        Path::new(PERF_EVENT_PARANOID_PATH),
        Path::new(KPTR_RESTRICT_PATH),
        enabled,
    )
}

fn configure_at(
    perf_event_paranoid_path: &Path,
    kptr_restrict_path: &Path,
    enabled: bool,
) -> Result<()> {
    if !enabled {
        return Ok(());
    }

    write_sysctl(perf_event_paranoid_path, "perf_event_paranoid", "-1\n")?;
    write_sysctl(kptr_restrict_path, "kptr_restrict", "0\n")
}

fn write_sysctl(path: &Path, setting: &str, value: &str) -> Result<()> {
    let display_value = value.trim_end();
    let mut file = OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(path)
        .with_context(|| {
            format!(
                "failed to open {} to set {setting}={display_value}",
                path.display()
            )
        })?;
    file.write_all(value.as_bytes())
        .with_context(|| format!("failed to set {} {setting}={display_value}", path.display()))
}

#[cfg(test)]
#[path = "perf_tests.rs"]
mod tests;
