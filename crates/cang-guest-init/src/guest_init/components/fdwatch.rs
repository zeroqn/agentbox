//! Guest-side file-descriptor pressure watchdog and report.
//!
//! A "Too many open files" failure inside a cang guest has two possible
//! owners: the fd table of the failing guest process, or the host libkrun VM
//! worker's fd table behind every virtiofs mount. libkrun forwards the host
//! errno verbatim (`virtio/fs/server.rs` `reply_error`), so the guest cannot
//! tell the two apart from the error alone. This module samples `/proc`,
//! records the worst consumers, and probes a guest-local target against a
//! virtiofs target to label the origin.

use anyhow::Result;
use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::Path;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::guest_init::components::env::FD_PRESSURE_STATUS_PATH;
use crate::guest_init::fs as guest_fs;

/// Cadence of the in-session pressure samples.
pub(in crate::guest_init) const DEFAULT_WATCH_INTERVAL: Duration = Duration::from_secs(10);
/// Soft-limit percentage at which a process is worth a first look.
const NOTICE_PERCENT: u32 = 50;
/// Soft-limit percentage that warns before the failing syscall.
const WARN_PERCENT: u32 = 75;
/// Soft-limit percentage that is one allocation burst away from EMFILE.
const CRITICAL_PERCENT: u32 = 90;
/// Growth between two samples that is a leak rather than a transient burst.
const GROWTH_ALERT_FDS: usize = 256;
/// Suppression window for repeated growth warnings about the same process.
const GROWTH_WARN_COOLDOWN: Duration = Duration::from_secs(300);
/// Process rows kept in the rendered report.
const MAX_REPORTED_PROCESSES: usize = 16;
const MAX_COMMAND_CHARS: usize = 96;
/// Guest-local probe target: opening it only spends a guest fd.
const LOCAL_PROBE_PATH: &str = "/dev/null";
/// Virtiofs probe target: opening it spends a guest fd and a host VM-worker fd.
const VIRTIOFS_PROBE_PATH: &str = "/workspace";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::guest_init) struct NofileLimits {
    pub(in crate::guest_init) soft: u64,
    pub(in crate::guest_init) hard: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(in crate::guest_init) enum FdPressureLevel {
    Ok,
    Notice,
    Warn,
    Critical,
}

impl FdPressureLevel {
    pub(in crate::guest_init) fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Notice => "notice",
            Self::Warn => "warn",
            Self::Critical => "critical",
        }
    }
}

fn level_for_percent(percent: u32) -> FdPressureLevel {
    if percent >= CRITICAL_PERCENT {
        FdPressureLevel::Critical
    } else if percent >= WARN_PERCENT {
        FdPressureLevel::Warn
    } else if percent >= NOTICE_PERCENT {
        FdPressureLevel::Notice
    } else {
        FdPressureLevel::Ok
    }
}

/// Which side of the VM boundary is out of descriptors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::guest_init) enum FdPressureOrigin {
    /// No exhaustion observed, or only on a target that cannot label an owner.
    Normal,
    /// A guest-local open failed with `EMFILE`/`ENFILE`.
    Guest,
    /// A guest-local open succeeded while a virtiofs open failed with
    /// `EMFILE`/`ENFILE`, which is the host VM worker's fd table.
    Host,
}

impl FdPressureOrigin {
    pub(in crate::guest_init) fn as_str(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Guest => "guest",
            Self::Host => "host",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(in crate::guest_init) struct FdTargetSummary {
    socket: usize,
    pipe: usize,
    anon_inode: usize,
    regular: usize,
    other: usize,
}

impl FdTargetSummary {
    fn render(&self) -> String {
        format!(
            "socket={} pipe={} anon_inode={} regular={} other={}",
            self.socket, self.pipe, self.anon_inode, self.regular, self.other
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::guest_init) struct ProcessFdSample {
    pub(in crate::guest_init) pid: u32,
    pub(in crate::guest_init) command: String,
    pub(in crate::guest_init) count: usize,
    pub(in crate::guest_init) limits: NofileLimits,
    pub(in crate::guest_init) targets: FdTargetSummary,
}

impl ProcessFdSample {
    /// Percentage of the soft limit in use, or `None` when the limit is
    /// unlimited or zero and a percentage would be meaningless.
    pub(in crate::guest_init) fn usage_percent(&self) -> Option<u32> {
        if self.limits.soft == 0 || self.limits.soft == u64::MAX {
            return None;
        }
        let percent = (self.count as u64).saturating_mul(100) / self.limits.soft;
        Some(u32::try_from(percent).unwrap_or(u32::MAX))
    }

    pub(in crate::guest_init) fn level(&self) -> FdPressureLevel {
        self.usage_percent()
            .map_or(FdPressureLevel::Ok, level_for_percent)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::guest_init) struct FdPressureReport {
    pub(in crate::guest_init) interval: Duration,
    pub(in crate::guest_init) updated_at: SystemTime,
    /// Processes ordered worst first.
    pub(in crate::guest_init) samples: Vec<ProcessFdSample>,
    /// Processes whose `/proc/<pid>/fd` or `/proc/<pid>/limits` was unreadable.
    pub(in crate::guest_init) unreadable: usize,
    pub(in crate::guest_init) system_allocated: Option<u64>,
    pub(in crate::guest_init) system_max: Option<u64>,
    pub(in crate::guest_init) origin: FdPressureOrigin,
}

impl FdPressureReport {
    pub(in crate::guest_init) fn worst_level(&self) -> FdPressureLevel {
        self.samples
            .iter()
            .map(ProcessFdSample::level)
            .max()
            .unwrap_or(FdPressureLevel::Ok)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::guest_init) enum FdPressureWarning {
    Level {
        pid: u32,
        command: String,
        level: FdPressureLevel,
        count: usize,
        soft_limit: u64,
    },
    Growth {
        pid: u32,
        command: String,
        count: usize,
        previous: usize,
    },
}

impl FdPressureWarning {
    pub(in crate::guest_init) fn render(&self) -> String {
        match self {
            Self::Level {
                pid,
                command,
                level,
                count,
                soft_limit,
            } => format!(
                "pid {pid} ({command}) is at {level} ({count} of {soft_limit} descriptors)",
                level = level.as_str()
            ),
            Self::Growth {
                pid,
                command,
                count,
                previous,
            } => format!(
                "pid {pid} ({command}) grew from {previous} to {count} descriptors since the last sample"
            ),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::guest_init) struct FdWatchTick {
    pub(in crate::guest_init) report: FdPressureReport,
    pub(in crate::guest_init) warnings: Vec<FdPressureWarning>,
}

/// Everything the sampler reads. Split out so tests can drive the sampler
/// without a real `/proc`.
pub(in crate::guest_init) trait FdEnv {
    fn pids(&self) -> Vec<u32>;
    fn command(&self, pid: u32) -> Option<String>;
    /// `None` when the fd directory cannot be read (for example another user's
    /// process).
    fn fd_count(&self, pid: u32) -> Option<usize>;
    fn fd_targets(&self, pid: u32) -> Vec<String>;
    fn nofile_limits(&self, pid: u32) -> Option<NofileLimits>;
    fn system_fd_usage(&self) -> Option<(u64, u64)>;
    fn probe_local(&self) -> io::Result<()>;
    fn probe_virtiofs(&self) -> io::Result<()>;
}

pub(in crate::guest_init) struct ProcFdEnv;

impl FdEnv for ProcFdEnv {
    fn pids(&self) -> Vec<u32> {
        let mut pids = Vec::new();
        let Ok(entries) = fs::read_dir("/proc") else {
            return pids;
        };
        for entry in entries.flatten() {
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if let Ok(pid) = name.parse::<u32>() {
                pids.push(pid);
            }
        }
        pids.sort_unstable();
        pids
    }

    fn command(&self, pid: u32) -> Option<String> {
        if let Ok(bytes) = fs::read(format!("/proc/{pid}/cmdline")) {
            let text = String::from_utf8_lossy(&bytes).replace('\0', " ");
            let text = sanitize_command(&text);
            if !text.is_empty() {
                return Some(text);
            }
        }
        let comm = fs::read_to_string(format!("/proc/{pid}/comm")).ok()?;
        let comm = sanitize_command(&comm);
        (!comm.is_empty()).then_some(comm)
    }

    fn fd_count(&self, pid: u32) -> Option<usize> {
        let entries = fs::read_dir(format!("/proc/{pid}/fd")).ok()?;
        Some(entries.flatten().count())
    }

    fn fd_targets(&self, pid: u32) -> Vec<String> {
        let mut targets = Vec::new();
        let Ok(entries) = fs::read_dir(format!("/proc/{pid}/fd")) else {
            return targets;
        };
        for entry in entries.flatten() {
            if let Ok(target) = fs::read_link(entry.path()) {
                targets.push(target.to_string_lossy().into_owned());
            }
        }
        targets
    }

    fn nofile_limits(&self, pid: u32) -> Option<NofileLimits> {
        parse_nofile_limits(&fs::read_to_string(format!("/proc/{pid}/limits")).ok()?)
    }

    fn system_fd_usage(&self) -> Option<(u64, u64)> {
        let allocated = parse_file_nr(&fs::read_to_string("/proc/sys/fs/file-nr").ok()?)?;
        let max = fs::read_to_string("/proc/sys/fs/file-max")
            .ok()?
            .trim()
            .parse()
            .ok()?;
        Some((allocated, max))
    }

    fn probe_local(&self) -> io::Result<()> {
        File::open(LOCAL_PROBE_PATH).map(|_| ())
    }

    fn probe_virtiofs(&self) -> io::Result<()> {
        File::open(VIRTIOFS_PROBE_PATH).map(|_| ())
    }
}

fn sanitize_command(text: &str) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= MAX_COMMAND_CHARS {
        return collapsed;
    }
    let mut truncated: String = collapsed.chars().take(MAX_COMMAND_CHARS).collect();
    truncated.push_str("...");
    truncated
}

fn parse_nofile_limits(text: &str) -> Option<NofileLimits> {
    for line in text.lines() {
        let Some(rest) = line.strip_prefix("Max open files") else {
            continue;
        };
        let mut fields = rest.split_whitespace();
        let soft = parse_limit(fields.next()?)?;
        let hard = parse_limit(fields.next()?)?;
        return Some(NofileLimits { soft, hard });
    }
    None
}

fn parse_limit(value: &str) -> Option<u64> {
    if value == "unlimited" {
        return Some(u64::MAX);
    }
    value.parse().ok()
}

fn parse_file_nr(text: &str) -> Option<u64> {
    text.split_whitespace().next()?.parse().ok()
}

fn summarize_targets(targets: &[String]) -> FdTargetSummary {
    let mut summary = FdTargetSummary::default();
    for target in targets {
        if target.starts_with("socket:") {
            summary.socket += 1;
        } else if target.starts_with("pipe:") {
            summary.pipe += 1;
        } else if target.starts_with("anon_inode:") {
            summary.anon_inode += 1;
        } else if target.starts_with('/') {
            summary.regular += 1;
        } else {
            summary.other += 1;
        }
    }
    summary
}

fn is_fd_exhaustion(err: &io::Error) -> bool {
    matches!(
        err.raw_os_error(),
        Some(code) if code == libc::EMFILE || code == libc::ENFILE
    )
}

fn classify_origin(local: &io::Result<()>, virtiofs: &io::Result<()>) -> FdPressureOrigin {
    if local.as_ref().err().is_some_and(is_fd_exhaustion) {
        return FdPressureOrigin::Guest;
    }
    if virtiofs.as_ref().err().is_some_and(is_fd_exhaustion) {
        return FdPressureOrigin::Host;
    }
    FdPressureOrigin::Normal
}

fn sort_key(sample: &ProcessFdSample) -> (FdPressureLevel, u32, usize) {
    (
        sample.level(),
        sample.usage_percent().unwrap_or(0),
        sample.count,
    )
}

fn sample(env: &impl FdEnv, interval: Duration, now: SystemTime) -> FdPressureReport {
    let mut samples = Vec::new();
    let mut unreadable = 0usize;
    for pid in env.pids() {
        let (Some(count), Some(limits)) = (env.fd_count(pid), env.nofile_limits(pid)) else {
            unreadable += 1;
            continue;
        };
        let targets = summarize_targets(&env.fd_targets(pid));
        samples.push(ProcessFdSample {
            pid,
            command: env.command(pid).unwrap_or_else(|| format!("pid {pid}")),
            count,
            limits,
            targets,
        });
    }
    samples.sort_by_key(|sample| Reverse(sort_key(sample)));

    let (system_allocated, system_max) = match env.system_fd_usage() {
        Some((allocated, max)) => (Some(allocated), Some(max)),
        None => (None, None),
    };
    let origin = classify_origin(&env.probe_local(), &env.probe_virtiofs());

    FdPressureReport {
        interval,
        updated_at: now,
        samples,
        unreadable,
        system_allocated,
        system_max,
        origin,
    }
}

/// Samples `/proc` on a fixed cadence and reports only when a sample is due.
pub(in crate::guest_init) struct FdWatch {
    interval: Duration,
    next_sample: Option<Instant>,
    previous_counts: BTreeMap<u32, usize>,
    last_levels: BTreeMap<u32, FdPressureLevel>,
    growth_warned_at: BTreeMap<u32, Instant>,
}

impl FdWatch {
    pub(in crate::guest_init) fn new(interval: Duration) -> Self {
        Self {
            interval,
            next_sample: None,
            previous_counts: BTreeMap::new(),
            last_levels: BTreeMap::new(),
            growth_warned_at: BTreeMap::new(),
        }
    }

    /// Returns `None` when the cadence has not elapsed yet.
    pub(in crate::guest_init) fn poll(
        &mut self,
        now: Instant,
        env: &impl FdEnv,
    ) -> Option<FdWatchTick> {
        if let Some(next) = self.next_sample
            && now < next
        {
            return None;
        }
        self.next_sample = Some(now + self.interval);
        let report = sample(env, self.interval, SystemTime::now());
        let warnings = self.evaluate(&report, now);
        Some(FdWatchTick { report, warnings })
    }

    fn evaluate(&mut self, report: &FdPressureReport, now: Instant) -> Vec<FdPressureWarning> {
        let mut warnings = Vec::new();
        for sample in &report.samples {
            let level = sample.level();
            if level
                > self
                    .last_levels
                    .insert(sample.pid, level)
                    .unwrap_or(FdPressureLevel::Ok)
            {
                warnings.push(FdPressureWarning::Level {
                    pid: sample.pid,
                    command: sample.command.clone(),
                    level,
                    count: sample.count,
                    soft_limit: sample.limits.soft,
                });
            }
            let previous = self.previous_counts.insert(sample.pid, sample.count);
            if let Some(previous) = previous
                && sample.count.saturating_sub(previous) >= GROWTH_ALERT_FDS
                && self.growth_warn_allowed(sample.pid, now)
            {
                warnings.push(FdPressureWarning::Growth {
                    pid: sample.pid,
                    command: sample.command.clone(),
                    count: sample.count,
                    previous,
                });
            }
        }

        let live: BTreeSet<u32> = report.samples.iter().map(|sample| sample.pid).collect();
        self.previous_counts.retain(|pid, _| live.contains(pid));
        self.last_levels.retain(|pid, _| live.contains(pid));
        self.growth_warned_at.retain(|pid, _| live.contains(pid));
        warnings
    }

    fn growth_warn_allowed(&mut self, pid: u32, now: Instant) -> bool {
        if let Some(at) = self.growth_warned_at.get(&pid)
            && now.duration_since(*at) < GROWTH_WARN_COOLDOWN
        {
            return false;
        }
        self.growth_warned_at.insert(pid, now);
        true
    }
}

fn unix_secs(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn render_limit(value: u64) -> String {
    if value == u64::MAX {
        "unlimited".to_owned()
    } else {
        value.to_string()
    }
}

/// Renders the `key=value` report written to `/run/cang/fd-pressure.status`
/// and printed by `cang-guest-init fd-report`.
pub(in crate::guest_init) fn render_report(report: &FdPressureReport) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "updated_at={}", unix_secs(report.updated_at));
    let _ = writeln!(out, "interval_secs={}", report.interval.as_secs());
    let _ = writeln!(out, "level={}", report.worst_level().as_str());
    let _ = writeln!(out, "origin={}", report.origin.as_str());
    if let (Some(allocated), Some(max)) = (report.system_allocated, report.system_max) {
        let _ = writeln!(out, "system_fds_allocated={allocated}");
        let _ = writeln!(out, "system_fds_max={max}");
    }
    let _ = writeln!(out, "processes={}", report.samples.len());
    let _ = writeln!(out, "unreadable={}", report.unreadable);
    for (index, sample) in report
        .samples
        .iter()
        .take(MAX_REPORTED_PROCESSES)
        .enumerate()
    {
        let _ = writeln!(out, "process.{index}.pid={}", sample.pid);
        let _ = writeln!(out, "process.{index}.command={}", sample.command);
        let _ = writeln!(out, "process.{index}.count={}", sample.count);
        let _ = writeln!(
            out,
            "process.{index}.soft_limit={}",
            render_limit(sample.limits.soft)
        );
        let _ = writeln!(
            out,
            "process.{index}.hard_limit={}",
            render_limit(sample.limits.hard)
        );
        if let Some(percent) = sample.usage_percent() {
            let _ = writeln!(out, "process.{index}.usage_percent={percent}");
        }
        let _ = writeln!(out, "process.{index}.level={}", sample.level().as_str());
        let _ = writeln!(out, "process.{index}.targets={}", sample.targets.render());
    }
    out
}

/// Writes the latest sample to the guest status file and prints warnings to
/// guest-init stderr. Failures are ignored: reporting must never fail a session.
pub(in crate::guest_init) fn record_and_warn(tick: &FdWatchTick) {
    let _ = guest_fs::write_file(
        Path::new(FD_PRESSURE_STATUS_PATH),
        &render_report(&tick.report),
        0o644,
    );
    for warning in &tick.warnings {
        eprintln!("cang-guest-init: fd pressure: {}", warning.render());
    }
}

/// `cang-guest-init fd-report` implementation.
pub(in crate::guest_init) fn run_report(watch: bool, interval: Duration) -> Result<()> {
    let env = ProcFdEnv;
    let mut stdout = io::stdout().lock();
    loop {
        let report = sample(&env, interval, SystemTime::now());
        stdout.write_all(render_report(&report).as_bytes())?;
        stdout.flush()?;
        if !watch {
            return Ok(());
        }
        stdout.write_all(b"\n")?;
        stdout.flush()?;
        std::thread::sleep(interval);
    }
}

#[cfg(test)]
#[path = "fdwatch_tests.rs"]
mod tests;
