use std::io::{self, Write};
use std::time::Duration;

pub(in crate::runtime::session) const ATTACH_PROFILE_ENV: &str = "LOFTD_ATTACH_PROFILE";

pub(in crate::runtime::session) fn enabled_from_process_env() -> bool {
    std::env::var(ATTACH_PROFILE_ENV)
        .ok()
        .as_deref()
        .is_some_and(env_value_enabled)
}

pub(in crate::runtime::session) fn guest_env_pair_from_process_env() -> Option<(String, String)> {
    guest_env_pair_from_value(std::env::var(ATTACH_PROFILE_ENV).ok().as_deref())
}

pub(in crate::runtime::session) fn guest_env_pair_from_value(
    value: Option<&str>,
) -> Option<(String, String)> {
    value
        .filter(|value| env_value_enabled(value))
        .map(|_| (ATTACH_PROFILE_ENV.to_owned(), "1".to_owned()))
}

fn env_value_enabled(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on" | "summary"
    )
}

#[derive(Debug, Default)]
pub(in crate::runtime::session) struct HostAttachProfiler {
    enabled: bool,
    frames: u64,
    bytes: u64,
    frame_max_bytes: usize,
    frame_read_total: Duration,
    frame_read_max: Duration,
    stdout_write_total: Duration,
    stdout_write_max: Duration,
    stdout_flush_total: Duration,
    stdout_flush_max: Duration,
}

impl HostAttachProfiler {
    pub(in crate::runtime::session) fn from_process_env() -> Self {
        Self::new(enabled_from_process_env())
    }

    pub(in crate::runtime::session) fn new(enabled: bool) -> Self {
        Self {
            enabled,
            ..Self::default()
        }
    }

    pub(in crate::runtime::session) fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub(in crate::runtime::session) fn record_frame_read(&mut self, duration: Duration) {
        if !self.enabled {
            return;
        }
        self.frame_read_total += duration;
        self.frame_read_max = self.frame_read_max.max(duration);
    }

    pub(in crate::runtime::session) fn record_data_frame(&mut self, bytes: usize) {
        if !self.enabled {
            return;
        }
        self.frames += 1;
        self.bytes = self.bytes.saturating_add(bytes as u64);
        self.frame_max_bytes = self.frame_max_bytes.max(bytes);
    }

    pub(in crate::runtime::session) fn record_stdout_write(&mut self, duration: Duration) {
        if !self.enabled {
            return;
        }
        self.stdout_write_total += duration;
        self.stdout_write_max = self.stdout_write_max.max(duration);
    }

    pub(in crate::runtime::session) fn record_stdout_flush(&mut self, duration: Duration) {
        if !self.enabled {
            return;
        }
        self.stdout_flush_total += duration;
        self.stdout_flush_max = self.stdout_flush_max.max(duration);
    }

    pub(in crate::runtime::session) fn report_to(&self, writer: &mut impl Write) -> io::Result<()> {
        if !self.enabled {
            return Ok(());
        }
        writer.write_all(self.summary_line().as_bytes())?;
        writer.write_all(b"\n")?;
        writer.flush()
    }

    fn summary_line(&self) -> String {
        format!(
            "loftd attach profile role=host frames={} bytes={} frame_max_bytes={} frame_avg_bytes={} frame_read_total_us={} frame_read_max_us={} stdout_write_total_us={} stdout_write_max_us={} stdout_flush_total_us={} stdout_flush_max_us={}",
            self.frames,
            self.bytes,
            self.frame_max_bytes,
            average(self.bytes, self.frames),
            duration_micros(self.frame_read_total),
            duration_micros(self.frame_read_max),
            duration_micros(self.stdout_write_total),
            duration_micros(self.stdout_write_max),
            duration_micros(self.stdout_flush_total),
            duration_micros(self.stdout_flush_max),
        )
    }
}

fn average(total: u64, count: u64) -> u64 {
    total.checked_div(count).unwrap_or(0)
}

fn duration_micros(duration: Duration) -> u128 {
    duration.as_micros()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attach_profile_env_pair_requires_truthy_value() {
        assert_eq!(guest_env_pair_from_value(None), None);
        assert_eq!(guest_env_pair_from_value(Some("")), None);
        assert_eq!(guest_env_pair_from_value(Some("0")), None);
        assert_eq!(guest_env_pair_from_value(Some("false")), None);
        assert_eq!(
            guest_env_pair_from_value(Some("summary")),
            Some((ATTACH_PROFILE_ENV.to_owned(), "1".to_owned()))
        );
    }

    #[test]
    fn host_attach_profile_summary_has_stable_shape() {
        let mut profiler = HostAttachProfiler::new(true);
        profiler.record_frame_read(Duration::from_micros(7));
        profiler.record_data_frame(4);
        profiler.record_stdout_write(Duration::from_micros(11));
        profiler.record_stdout_flush(Duration::from_micros(13));
        let mut output = Vec::new();

        profiler.report_to(&mut output).unwrap();

        let output = String::from_utf8(output).unwrap();
        assert!(output.starts_with("loftd attach profile role=host "));
        assert!(output.contains("frames=1"));
        assert!(output.contains("bytes=4"));
        assert!(output.contains("frame_max_bytes=4"));
        assert!(output.contains("frame_avg_bytes=4"));
        assert!(output.contains("frame_read_total_us=7"));
        assert!(output.contains("stdout_write_total_us=11"));
        assert!(output.contains("stdout_flush_total_us=13"));
    }

    #[test]
    fn disabled_host_attach_profile_is_quiet() {
        let profiler = HostAttachProfiler::new(false);
        let mut output = Vec::new();

        profiler.report_to(&mut output).unwrap();

        assert!(output.is_empty());
    }
}
