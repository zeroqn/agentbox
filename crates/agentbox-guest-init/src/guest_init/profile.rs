use anyhow::{Context, Result};
use std::env;
use std::io::{self, Write};
use std::time::{Duration, Instant};

pub(in crate::guest_init) const GUEST_PROFILE_ENV: &str = "AGENTBOX_GUEST_PROFILE";
pub(in crate::guest_init) const GUEST_DEBUG_ENV: &str = "AGENTBOX_GUEST_DEBUG";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ProfileConfig {
    enabled: bool,
    debug: bool,
}

#[derive(Debug)]
struct ProfileRecord {
    label: &'static str,
    duration: Duration,
}

pub(in crate::guest_init) struct GuestProfiler {
    config: ProfileConfig,
    section: &'static str,
    started_at: Instant,
    records: Vec<ProfileRecord>,
}

pub(in crate::guest_init) trait ProfileRecorder {
    fn measure_result<T, F>(&mut self, label: &'static str, f: F) -> Result<T>
    where
        F: FnOnce() -> Result<T>;
}

trait EnvSource {
    fn var(&self, name: &str) -> Option<String>;
}

struct ProcessEnv;

impl EnvSource for ProcessEnv {
    fn var(&self, name: &str) -> Option<String> {
        env::var(name).ok().filter(|value| !value.is_empty())
    }
}

impl ProfileConfig {
    fn from_env(env: &impl EnvSource) -> Self {
        Self {
            enabled: env.var(GUEST_PROFILE_ENV).as_deref() == Some("1"),
            debug: env.var(GUEST_DEBUG_ENV).as_deref() == Some("1"),
        }
    }

    fn should_report(self) -> bool {
        self.enabled && self.debug
    }
}

impl GuestProfiler {
    pub(in crate::guest_init) fn from_process_env(section: &'static str) -> Self {
        Self::new(section, ProfileConfig::from_env(&ProcessEnv))
    }

    fn new(section: &'static str, config: ProfileConfig) -> Self {
        Self {
            config,
            section,
            started_at: Instant::now(),
            records: Vec::new(),
        }
    }

    pub(in crate::guest_init) fn measure<T>(
        &mut self,
        label: &'static str,
        f: impl FnOnce() -> T,
    ) -> T {
        if !self.config.enabled {
            return f();
        }

        let started_at = Instant::now();
        let value = f();
        self.records.push(ProfileRecord {
            label,
            duration: started_at.elapsed(),
        });
        value
    }

    pub(in crate::guest_init) fn measure_result<T>(
        &mut self,
        label: &'static str,
        f: impl FnOnce() -> Result<T>,
    ) -> Result<T> {
        if !self.config.enabled {
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

    #[cfg(test)]
    pub(in crate::guest_init) fn measure_result_with_profiler<T>(
        &mut self,
        label: &'static str,
        f: impl FnOnce(&mut Self) -> Result<T>,
    ) -> Result<T> {
        if !self.config.enabled {
            return f(self);
        }

        let insertion_index = self.records.len();
        let started_at = Instant::now();
        let result = f(self);
        self.records.insert(
            insertion_index,
            ProfileRecord {
                label,
                duration: started_at.elapsed(),
            },
        );
        result
    }

    pub(in crate::guest_init) fn report_before_exec(&self) -> Result<()> {
        let stderr = io::stderr();
        let mut writer = stderr.lock();
        self.write_report_with_total(&mut writer, self.started_at.elapsed())
            .context("failed to write guest-init profile report")
    }

    fn write_report_with_total(&self, writer: &mut impl Write, total: Duration) -> io::Result<()> {
        if !self.config.should_report() {
            return Ok(());
        }

        writeln!(writer, "agentbox-guest-init profile: {}", self.section)?;
        for record in &self.records {
            writeln!(
                writer,
                "  {}: {}",
                record.label,
                format_duration(record.duration)
            )?;
        }
        writeln!(writer, "  total: {}", format_duration(total))?;
        writer.flush()
    }
}

impl ProfileRecorder for GuestProfiler {
    fn measure_result<T, F>(&mut self, label: &'static str, f: F) -> Result<T>
    where
        F: FnOnce() -> Result<T>,
    {
        GuestProfiler::measure_result(self, label, f)
    }
}

pub(in crate::guest_init) fn clear_guest_profile_env() {
    env::remove_var(GUEST_PROFILE_ENV);
    env::remove_var(GUEST_DEBUG_ENV);
}

fn format_duration(duration: Duration) -> String {
    format!("{:.3}ms", duration.as_secs_f64() * 1000.0)
}

#[cfg(test)]
#[path = "profile_tests.rs"]
mod tests;
