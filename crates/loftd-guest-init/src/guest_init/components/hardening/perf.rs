use anyhow::{Context, Result};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;

const PERF_EVENT_PARANOID_PATH: &str = "/proc/sys/kernel/perf_event_paranoid";

pub(in crate::guest_init) fn configure(enabled: bool) -> Result<()> {
    configure_at(Path::new(PERF_EVENT_PARANOID_PATH), enabled)
}

fn configure_at(path: &Path, enabled: bool) -> Result<()> {
    if !enabled {
        return Ok(());
    }

    let value = "-1\n";
    let setting = value.trim_end();
    let mut file = OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(path)
        .with_context(|| {
            format!(
                "failed to open {} to set perf_event_paranoid={setting}",
                path.display()
            )
        })?;
    file.write_all(value.as_bytes()).with_context(|| {
        format!(
            "failed to set {} perf_event_paranoid={setting}",
            path.display()
        )
    })
}

#[cfg(test)]
#[path = "perf_tests.rs"]
mod tests;
