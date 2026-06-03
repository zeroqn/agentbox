use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use super::{
    EnvSource, GUEST_DEBUG_ENV, GUEST_PROFILE_ENV, GuestProfiler, ProfileConfig, ProfileRecord,
};

struct TestEnv(BTreeMap<String, String>);

impl TestEnv {
    fn new(vars: &[(&str, &str)]) -> Self {
        Self(
            vars.iter()
                .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
                .collect(),
        )
    }
}

impl EnvSource for TestEnv {
    fn var(&self, name: &str) -> Option<String> {
        self.0.get(name).cloned()
    }
}

#[test]
fn profile_config_requires_explicit_profile_env_to_enable_measurement() {
    assert!(!ProfileConfig::from_env(&TestEnv::new(&[])).enabled);
    assert!(!ProfileConfig::from_env(&TestEnv::new(&[(GUEST_PROFILE_ENV, "0")])).enabled);
    assert!(ProfileConfig::from_env(&TestEnv::new(&[(GUEST_PROFILE_ENV, "1")])).enabled);
}

#[test]
fn profile_config_reports_only_when_profile_and_debug_are_enabled() {
    assert!(!ProfileConfig::from_env(&TestEnv::new(&[(GUEST_PROFILE_ENV, "1")])).should_report());
    assert!(!ProfileConfig::from_env(&TestEnv::new(&[(GUEST_DEBUG_ENV, "1")])).should_report());
    assert!(
        ProfileConfig::from_env(&TestEnv::new(&[
            (GUEST_PROFILE_ENV, "1"),
            (GUEST_DEBUG_ENV, "1"),
        ]))
        .should_report()
    );
}

#[test]
fn disabled_profiler_does_not_record_measurements() {
    let mut profiler = GuestProfiler::new(
        "container enter",
        ProfileConfig {
            enabled: false,
            debug: true,
        },
    );

    let value = profiler.measure("derive-identity", || 42);

    assert_eq!(value, 42);
    assert!(profiler.records.is_empty());
}

#[test]
fn enabled_profiler_records_successful_and_failed_measurements() {
    let mut profiler = GuestProfiler::new(
        "loftd enter",
        ProfileConfig {
            enabled: true,
            debug: false,
        },
    );

    profiler.measure("read-env", || ());
    let err = profiler
        .measure_result::<()>("bootstrap-nix", || anyhow::bail!("boom"))
        .expect_err("failed component should be returned");

    assert_eq!(profiler.records.len(), 2);
    assert_eq!(profiler.records[0].label, "read-env");
    assert_eq!(profiler.records[1].label, "bootstrap-nix");
    assert_eq!(err.to_string(), "boom");
}

#[test]
fn nested_measurement_records_outer_before_inner_rows() {
    let mut profiler = GuestProfiler::new(
        "loftd enter",
        ProfileConfig {
            enabled: true,
            debug: true,
        },
    );

    profiler
        .measure_result_with_profiler("bootstrap-nix", |profiler| {
            profiler.measure_result("bootstrap-nix:find-disk", || Ok(()))?;
            profiler.measure_result("bootstrap-nix:wait-socket", || Ok(()))?;
            Ok(())
        })
        .expect("nested measurement should succeed");

    assert_eq!(profiler.records.len(), 3);
    assert_eq!(profiler.records[0].label, "bootstrap-nix");
    assert_eq!(profiler.records[1].label, "bootstrap-nix:find-disk");
    assert_eq!(profiler.records[2].label, "bootstrap-nix:wait-socket");
}

#[test]
fn failed_nested_measurement_records_outer_and_inner_rows() {
    let mut profiler = GuestProfiler::new(
        "loftd enter",
        ProfileConfig {
            enabled: true,
            debug: true,
        },
    );

    let err = profiler
        .measure_result_with_profiler::<()>("bootstrap-nix", |profiler| {
            profiler.measure_result("bootstrap-nix:wait-socket", || anyhow::bail!("timeout"))?;
            Ok(())
        })
        .expect_err("nested failure should be returned");

    assert_eq!(err.to_string(), "timeout");
    assert_eq!(profiler.records.len(), 2);
    assert_eq!(profiler.records[0].label, "bootstrap-nix");
    assert_eq!(profiler.records[1].label, "bootstrap-nix:wait-socket");
}

#[test]
fn disabled_nested_measurement_does_not_record_outer_or_inner_rows() {
    let mut profiler = GuestProfiler::new(
        "loftd enter",
        ProfileConfig {
            enabled: false,
            debug: true,
        },
    );

    profiler
        .measure_result_with_profiler("bootstrap-nix", |profiler| {
            profiler.measure_result("bootstrap-nix:wait-socket", || Ok(()))
        })
        .expect("disabled nested measurement should still run");

    assert!(profiler.records.is_empty());
}

#[test]
fn report_format_is_stable_and_uses_milliseconds() {
    let profiler = GuestProfiler {
        config: ProfileConfig {
            enabled: true,
            debug: true,
        },
        section: "loftd enter",
        started_at: Instant::now(),
        records: vec![
            ProfileRecord {
                label: "read-env",
                duration: Duration::from_micros(1_250),
            },
            ProfileRecord {
                label: "bootstrap-nix",
                duration: Duration::from_micros(2_500),
            },
        ],
    };
    let mut out = Vec::new();

    profiler
        .write_report_with_total(&mut out, Duration::from_micros(10_000))
        .expect("report should format");

    assert_eq!(
        String::from_utf8(out).expect("report should be utf8"),
        "loftd-guest-init profile: loftd enter\n  read-env: 1.250ms\n  bootstrap-nix: 2.500ms\n  total: 10.000ms\n"
    );
}

#[test]
fn report_is_suppressed_without_debug_even_when_measurements_exist() {
    let profiler = GuestProfiler {
        config: ProfileConfig {
            enabled: true,
            debug: false,
        },
        section: "container enter",
        started_at: Instant::now(),
        records: vec![ProfileRecord {
            label: "derive-identity",
            duration: Duration::from_millis(1),
        }],
    };
    let mut out = Vec::new();

    profiler
        .write_report_with_total(&mut out, Duration::from_millis(2))
        .expect("suppressed report should still succeed");

    assert!(out.is_empty());
}
