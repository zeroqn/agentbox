use anyhow::Result;
use std::io::{self, Write};
use std::time::{Duration, Instant};

#[derive(Debug)]
pub(super) struct LoftdHostProfiler {
    enabled: bool,
    started_at: Instant,
    metadata: Vec<ProfileMetadata>,
    records: Vec<ProfileRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProfileMetadata {
    label: &'static str,
    value: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ProfileRecord {
    label: &'static str,
    duration: Duration,
}

impl LoftdHostProfiler {
    pub(super) fn new_started_at(enabled: bool, started_at: Instant) -> Self {
        Self {
            enabled,
            started_at,
            metadata: Vec::new(),
            records: Vec::new(),
        }
    }

    pub(super) fn record_metadata(&mut self, label: &'static str, value: impl Into<String>) {
        if self.enabled {
            self.metadata.push(ProfileMetadata {
                label,
                value: value.into(),
            });
        }
    }

    pub(super) fn measure_result<T>(
        &mut self,
        label: &'static str,
        f: impl FnOnce() -> Result<T>,
    ) -> Result<T> {
        if !self.enabled {
            return f();
        }

        let started_at = Instant::now();
        let result = f();
        self.records.push(ProfileRecord {
            label,
            duration: started_at.elapsed(),
        });
        result
    }

    pub(super) fn emit_to_stderr(&self) {
        let stderr = io::stderr();
        let mut writer = stderr.lock();
        let _ = self.write_report(&mut writer);
    }

    fn write_report(&self, writer: &mut impl Write) -> io::Result<()> {
        self.write_report_with_total(writer, self.started_at.elapsed())
    }

    fn write_report_with_total(&self, writer: &mut impl Write, total: Duration) -> io::Result<()> {
        if !self.enabled {
            return Ok(());
        }

        writeln!(writer, "loftd host profile")?;
        for metadata in &self.metadata {
            writeln!(writer, "  {}: {}", metadata.label, metadata.value)?;
        }
        for record in &self.records {
            writeln!(
                writer,
                "  {}: {}",
                record.label,
                format_duration(record.duration)
            )?;
        }
        writeln!(
            writer,
            "  total_profiled_host_runtime: {}",
            format_duration(total)
        )?;
        writer.flush()
    }
}

fn format_duration(duration: Duration) -> String {
    format!("{:.3}ms", duration.as_secs_f64() * 1000.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::anyhow;

    #[test]
    fn host_profile_report_is_suppressed_when_disabled() {
        let profiler = LoftdHostProfiler::new_started_at(false, Instant::now());
        let mut output = Vec::new();

        profiler
            .write_report_with_total(&mut output, Duration::from_millis(1))
            .expect("disabled report should be writable");

        assert!(output.is_empty());
    }

    #[test]
    fn host_profile_report_uses_stable_labels_and_total_scope() {
        let mut profiler = LoftdHostProfiler::new_started_at(true, Instant::now());

        profiler.record_metadata("task_rootfs_backend", "btrfs-snapshot");
        profiler.record_metadata("image", "localhost/loftd:latest");
        profiler.record_metadata("image_digest", "sha256:abc123");
        profiler
            .measure_result("workspace_canonicalization", || Ok(()))
            .expect("phase should pass");
        profiler
            .measure_result("launch_plan_build", || Ok(()))
            .expect("phase should pass");
        profiler
            .measure_result("task_rootfs_materialization", || Ok(()))
            .expect("phase should pass");
        profiler
            .measure_result("persistent_disk_preparation", || Ok(()))
            .expect("phase should pass");
        profiler
            .measure_result("guest_init_resolution", || Ok(()))
            .expect("phase should pass");
        profiler
            .measure_result("launch_config_build", || Ok(()))
            .expect("phase should pass");
        profiler
            .measure_result("helper_session", || Ok(()))
            .expect("phase should pass");
        profiler
            .measure_result("task_state_cleanup", || Ok(()))
            .expect("phase should pass");

        let mut output = Vec::new();
        profiler
            .write_report_with_total(&mut output, Duration::from_millis(12))
            .expect("report should write");
        let text = String::from_utf8(output).expect("report should be utf-8");

        assert!(text.starts_with("loftd host profile\n"));
        assert!(text.contains("task_rootfs_backend: btrfs-snapshot"));
        assert!(text.contains("image: localhost/loftd:latest"));
        assert!(text.contains("image_digest: sha256:abc123"));
        assert!(!text.contains("loftd-guest-init profile"));
        for label in [
            "workspace_canonicalization",
            "launch_plan_build",
            "task_rootfs_materialization",
            "persistent_disk_preparation",
            "guest_init_resolution",
            "launch_config_build",
            "helper_session",
            "task_state_cleanup",
            "total_profiled_host_runtime",
        ] {
            assert!(text.contains(label), "missing profile label {label}");
        }
    }

    #[test]
    fn host_profile_total_uses_injected_session_start() {
        let started_at = Instant::now() - Duration::from_millis(123);
        let profiler = LoftdHostProfiler::new_started_at(true, started_at);
        let mut output = Vec::new();

        profiler
            .write_report(&mut output)
            .expect("report should write");
        let text = String::from_utf8(output).expect("report should be utf-8");
        let total_ms = text
            .lines()
            .find_map(|line| line.trim().strip_prefix("total_profiled_host_runtime: "))
            .and_then(|value| value.strip_suffix("ms"))
            .and_then(|value| value.parse::<f64>().ok())
            .expect("total runtime should be present as milliseconds");

        assert!(
            total_ms >= 123.0,
            "total runtime should include time before profiler construction: {total_ms}ms"
        );
    }

    #[test]
    fn host_profile_records_failed_phases_before_returning_error() {
        let mut profiler = LoftdHostProfiler::new_started_at(true, Instant::now());

        let err = profiler
            .measure_result::<()>("helper_session", || Err(anyhow!("fake failure")))
            .expect_err("phase should fail");

        assert_eq!(err.to_string(), "fake failure");
        let mut output = Vec::new();
        profiler
            .write_report_with_total(&mut output, Duration::from_millis(1))
            .expect("report should write");
        let text = String::from_utf8(output).expect("report should be utf-8");
        assert!(text.contains("helper_session"));
    }
}
