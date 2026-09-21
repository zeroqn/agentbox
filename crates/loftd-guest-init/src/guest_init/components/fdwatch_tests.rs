use std::collections::BTreeMap;
use std::io;
use std::time::{Duration, Instant, UNIX_EPOCH};

use super::*;

#[derive(Debug)]
struct FakeProcess {
    command: String,
    targets: Vec<String>,
    limits: NofileLimits,
    readable: bool,
}

fn repeated(target: &str, count: usize) -> Vec<String> {
    (0..count).map(|_| target.to_owned()).collect()
}

fn readable_process(command: &str, count: usize, soft: u64) -> FakeProcess {
    FakeProcess {
        command: command.to_owned(),
        targets: repeated("/nix/store/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-x", count),
        limits: NofileLimits { soft, hard: soft },
        readable: true,
    }
}

#[derive(Debug, Default)]
struct FakeFdEnv {
    processes: BTreeMap<u32, FakeProcess>,
    system: Option<(u64, u64)>,
    local_probe_errno: Option<i32>,
    virtiofs_probe_errno: Option<i32>,
}

fn probe_result(errno: Option<i32>) -> io::Result<()> {
    match errno {
        Some(code) => Err(io::Error::from_raw_os_error(code)),
        None => Ok(()),
    }
}

impl FdEnv for FakeFdEnv {
    fn pids(&self) -> Vec<u32> {
        self.processes.keys().copied().collect()
    }

    fn command(&self, pid: u32) -> Option<String> {
        self.processes
            .get(&pid)
            .map(|process| process.command.clone())
    }

    fn fd_count(&self, pid: u32) -> Option<usize> {
        let process = self.processes.get(&pid)?;
        process.readable.then_some(process.targets.len())
    }

    fn fd_targets(&self, pid: u32) -> Vec<String> {
        self.processes
            .get(&pid)
            .map(|process| process.targets.clone())
            .unwrap_or_default()
    }

    fn nofile_limits(&self, pid: u32) -> Option<NofileLimits> {
        self.processes.get(&pid).map(|process| process.limits)
    }

    fn system_fd_usage(&self) -> Option<(u64, u64)> {
        self.system
    }

    fn probe_local(&self) -> io::Result<()> {
        probe_result(self.local_probe_errno)
    }

    fn probe_virtiofs(&self) -> io::Result<()> {
        probe_result(self.virtiofs_probe_errno)
    }
}

fn sample_of(count: usize, soft: u64) -> ProcessFdSample {
    ProcessFdSample {
        pid: 1,
        command: "probe".to_owned(),
        count,
        limits: NofileLimits { soft, hard: soft },
        targets: FdTargetSummary::default(),
    }
}

fn report_with(samples: Vec<ProcessFdSample>, origin: FdPressureOrigin) -> FdPressureReport {
    FdPressureReport {
        interval: Duration::from_secs(10),
        updated_at: UNIX_EPOCH + Duration::from_secs(1_757_000_000),
        samples,
        unreadable: 2,
        system_allocated: Some(2048),
        system_max: Some(9_223_372_036_854_775_807),
        origin,
    }
}

#[test]
fn levels_follow_soft_limit_percentages() {
    assert_eq!(sample_of(49, 100).level(), FdPressureLevel::Ok);
    assert_eq!(sample_of(50, 100).level(), FdPressureLevel::Notice);
    assert_eq!(sample_of(74, 100).level(), FdPressureLevel::Notice);
    assert_eq!(sample_of(75, 100).level(), FdPressureLevel::Warn);
    assert_eq!(sample_of(89, 100).level(), FdPressureLevel::Warn);
    assert_eq!(sample_of(90, 100).level(), FdPressureLevel::Critical);
    assert_eq!(sample_of(100, 100).level(), FdPressureLevel::Critical);
}

#[test]
fn unlimited_soft_limit_has_no_usage_percent() {
    let sample = sample_of(1000, u64::MAX);
    assert_eq!(sample.usage_percent(), None);
    assert_eq!(sample.level(), FdPressureLevel::Ok);
    assert_eq!(sample_of(1, 0).usage_percent(), None);
}

#[test]
fn parses_numeric_and_unlimited_nofile_limits() {
    let limits = "\
Limit                     Soft Limit           Hard Limit           Units     \n\
Max cpu time              unlimited            unlimited            seconds   \n\
Max open files            1024                 1048576              files     \n\
Max locked memory         65536                65536                bytes     \n";
    assert_eq!(
        parse_nofile_limits(limits),
        Some(NofileLimits {
            soft: 1024,
            hard: 1_048_576,
        })
    );

    let unlimited =
        "Max open files            unlimited            unlimited            files     \n";
    assert_eq!(
        parse_nofile_limits(unlimited),
        Some(NofileLimits {
            soft: u64::MAX,
            hard: u64::MAX,
        })
    );
    assert_eq!(
        parse_nofile_limits("Max cpu time unlimited unlimited seconds\n"),
        None
    );
}

#[test]
fn parses_allocated_system_fd_count() {
    assert_eq!(parse_file_nr("1234\t0\t9223372036854775807\n"), Some(1234));
    assert_eq!(parse_file_nr(""), None);
    assert_eq!(parse_file_nr("not-a-number 0 1\n"), None);
}

#[test]
fn summarizes_fd_target_kinds() {
    let targets = [
        "socket:[1234]".to_owned(),
        "socket:[5678]".to_owned(),
        "pipe:[99]".to_owned(),
        "anon_inode:[eventfd]".to_owned(),
        "/nix/store/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-x/bin/pi".to_owned(),
        "foo".to_owned(),
    ];
    let summary = summarize_targets(&targets);
    let rendered = summary.render();
    assert_eq!(rendered, "socket=2 pipe=1 anon_inode=1 regular=1 other=1");
}

#[test]
fn sanitize_command_collapses_whitespace_and_truncates() {
    assert_eq!(sanitize_command("  pi   --help  \n"), "pi --help");
    let long = "x".repeat(MAX_COMMAND_CHARS + 20);
    let sanitized = sanitize_command(&long);
    assert_eq!(sanitized.chars().count(), MAX_COMMAND_CHARS + 3);
    assert!(sanitized.ends_with("..."));
}

#[test]
fn classifies_guest_origin_when_local_probe_is_exhausted() {
    let env = FakeFdEnv {
        local_probe_errno: Some(libc::EMFILE),
        ..FakeFdEnv::default()
    };
    let report = sample(&env, DEFAULT_WATCH_INTERVAL, UNIX_EPOCH);
    assert_eq!(report.origin, FdPressureOrigin::Guest);
}

#[test]
fn classifies_host_origin_when_only_virtiofs_probe_is_exhausted() {
    let env = FakeFdEnv {
        virtiofs_probe_errno: Some(libc::EMFILE),
        ..FakeFdEnv::default()
    };
    let report = sample(&env, DEFAULT_WATCH_INTERVAL, UNIX_EPOCH);
    assert_eq!(report.origin, FdPressureOrigin::Host);
}

#[test]
fn origin_stays_normal_for_unrelated_probe_failures() {
    let env = FakeFdEnv {
        local_probe_errno: Some(libc::ENOENT),
        virtiofs_probe_errno: Some(libc::ENOTDIR),
        ..FakeFdEnv::default()
    };
    let report = sample(&env, DEFAULT_WATCH_INTERVAL, UNIX_EPOCH);
    assert_eq!(report.origin, FdPressureOrigin::Normal);

    let exhausted = Err(io::Error::from_raw_os_error(libc::ENFILE));
    assert_eq!(classify_origin(&Ok(()), &exhausted), FdPressureOrigin::Host);
}

#[test]
fn sample_orders_worst_first_and_counts_unreadable_processes() {
    let mut env = FakeFdEnv::default();
    env.processes.insert(4, readable_process("quiet", 10, 100));
    env.processes.insert(7, readable_process("pi", 80, 100));
    env.processes.insert(
        9,
        FakeProcess {
            command: "root-owned".to_owned(),
            targets: Vec::new(),
            limits: NofileLimits {
                soft: 100,
                hard: 100,
            },
            readable: false,
        },
    );
    env.system = Some((2048, 4096));

    let report = sample(&env, DEFAULT_WATCH_INTERVAL, UNIX_EPOCH);
    let pids: Vec<u32> = report.samples.iter().map(|sample| sample.pid).collect();
    assert_eq!(pids, vec![7, 4]);
    assert_eq!(report.unreadable, 1);
    assert_eq!(report.system_allocated, Some(2048));
    assert_eq!(report.system_max, Some(4096));
    assert_eq!(report.worst_level(), FdPressureLevel::Warn);
}

#[test]
fn watch_samples_on_the_configured_cadence() {
    let mut env = FakeFdEnv::default();
    env.processes.insert(7, readable_process("pi", 10, 100));
    let mut watch = FdWatch::new(Duration::from_secs(10));
    let start = Instant::now();

    assert!(watch.poll(start, &env).is_some());
    assert!(watch.poll(start + Duration::from_secs(9), &env).is_none());
    assert!(watch.poll(start + Duration::from_secs(10), &env).is_some());
}

#[test]
fn watch_warns_once_per_level_increase() {
    let mut env = FakeFdEnv::default();
    env.processes.insert(7, readable_process("pi", 10, 100));
    let mut watch = FdWatch::new(Duration::from_secs(1));
    let start = Instant::now();

    let baseline = watch.poll(start, &env).expect("first sample is due");
    assert!(baseline.warnings.is_empty());

    env.processes.insert(7, readable_process("pi", 80, 100));
    let warned = watch
        .poll(start + Duration::from_secs(1), &env)
        .expect("sample due");
    assert_eq!(warned.warnings.len(), 1);
    assert!(matches!(
        &warned.warnings[0],
        FdPressureWarning::Level {
            pid: 7,
            level: FdPressureLevel::Warn,
            count: 80,
            soft_limit: 100,
            ..
        }
    ));
    let rendered = warned.warnings[0].render();
    assert!(
        rendered.contains("pid 7 (pi) is at warn (80 of 100 descriptors)"),
        "unexpected warning text: {rendered}"
    );

    let repeat = watch
        .poll(start + Duration::from_secs(2), &env)
        .expect("sample due");
    assert!(
        repeat.warnings.is_empty(),
        "an unchanged level must not warn again"
    );
}

#[test]
fn watch_warns_on_growth_and_suppresses_repeats_inside_the_cooldown() {
    let mut env = FakeFdEnv::default();
    env.processes
        .insert(7, readable_process("pi", 100, 100_000_000));
    let mut watch = FdWatch::new(Duration::from_secs(1));
    let start = Instant::now();

    watch.poll(start, &env).expect("first sample is due");

    env.processes.insert(
        7,
        readable_process("pi", 100 + GROWTH_ALERT_FDS, 100_000_000),
    );
    let grown = watch
        .poll(start + Duration::from_secs(1), &env)
        .expect("sample due");
    assert_eq!(grown.warnings.len(), 1);
    assert!(matches!(
        &grown.warnings[0],
        FdPressureWarning::Growth {
            pid: 7,
            count: 356,
            previous: 100,
            ..
        }
    ));

    env.processes.insert(
        7,
        readable_process("pi", 100 + GROWTH_ALERT_FDS * 2, 100_000_000),
    );
    let suppressed = watch
        .poll(start + Duration::from_secs(2), &env)
        .expect("sample due");
    assert!(
        suppressed.warnings.is_empty(),
        "growth warnings inside the cooldown must be suppressed"
    );

    env.processes.insert(
        7,
        readable_process("pi", 100 + GROWTH_ALERT_FDS * 3, 100_000_000),
    );
    let later = watch
        .poll(start + GROWTH_WARN_COOLDOWN + Duration::from_secs(2), &env)
        .expect("sample due");
    assert_eq!(later.warnings.len(), 1);
}

#[test]
fn renders_key_value_report_rows() {
    let mut sample = sample_of(80, 100);
    sample.pid = 7;
    sample.command = "pi --help".to_owned();
    sample.limits = NofileLimits {
        soft: 100,
        hard: 200,
    };
    sample.targets = summarize_targets(&[
        "socket:[1]".to_owned(),
        "pipe:[2]".to_owned(),
        "anon_inode:[3]".to_owned(),
        "/x".to_owned(),
        "weird".to_owned(),
    ]);

    let report = report_with(vec![sample], FdPressureOrigin::Host);
    assert_eq!(
        render_report(&report),
        "\
updated_at=1757000000
interval_secs=10
level=warn
origin=host
system_fds_allocated=2048
system_fds_max=9223372036854775807
processes=1
unreadable=2
process.0.pid=7
process.0.command=pi --help
process.0.count=80
process.0.soft_limit=100
process.0.hard_limit=200
process.0.usage_percent=80
process.0.level=warn
process.0.targets=socket=1 pipe=1 anon_inode=1 regular=1 other=1
"
    );
}

#[test]
fn render_omits_unknown_counters_and_unlimited_usage() {
    let sample = sample_of(1000, u64::MAX);
    let mut report = report_with(vec![sample], FdPressureOrigin::Normal);
    report.system_allocated = None;
    report.system_max = None;
    report.unreadable = 0;

    let rendered = render_report(&report);
    assert!(!rendered.contains("system_fds_allocated"));
    assert!(!rendered.contains("system_fds_max"));
    assert!(!rendered.contains("usage_percent"));
    assert!(rendered.contains("process.0.soft_limit=unlimited\n"));
    assert!(rendered.contains("level=ok\n"));
}

#[test]
fn render_caps_reported_process_rows() {
    let samples: Vec<ProcessFdSample> = (0..MAX_REPORTED_PROCESSES + 4)
        .map(|index| {
            let mut sample = sample_of(10, 100);
            sample.pid = index as u32;
            sample
        })
        .collect();
    let rendered = render_report(&report_with(samples, FdPressureOrigin::Normal));

    let last_reported = MAX_REPORTED_PROCESSES - 1;
    assert!(rendered.contains(&format!("process.{last_reported}.pid={last_reported}\n")));
    assert!(!rendered.contains(&format!("process.{MAX_REPORTED_PROCESSES}.pid=")));
    assert!(rendered.contains(&format!("processes={}\n", MAX_REPORTED_PROCESSES + 4)));
}
