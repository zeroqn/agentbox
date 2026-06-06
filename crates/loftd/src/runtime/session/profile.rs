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
    pub(super) fn new(enabled: bool) -> Self {
        Self {
            enabled,
            started_at: Instant::now(),
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
        let _ = self.write_report_with_total(&mut writer, self.started_at.elapsed());
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
        let profiler = LoftdHostProfiler::new(false);
        let mut output = Vec::new();

        profiler
            .write_report_with_total(&mut output, Duration::from_millis(1))
            .expect("disabled report should be writable");

        assert!(output.is_empty());
    }

    #[test]
    fn host_profile_report_uses_stable_labels_and_total_scope() {
        let mut profiler = LoftdHostProfiler::new(true);

        profiler.record_metadata("task_rootfs_backend", "btrfs-snapshot");
        profiler.record_metadata("image", "localhost/loftd:latest");
        profiler.record_metadata("image_digest", "sha256:abc123");
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
        for label in [
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
    fn host_profile_records_failed_phases_before_returning_error() {
        let mut profiler = LoftdHostProfiler::new(true);

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
