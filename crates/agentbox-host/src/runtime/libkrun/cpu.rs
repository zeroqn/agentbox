use anyhow::{Context, Result};

use crate::podman::run::{RunArgOwner, RunSpec};

pub(crate) const CPU_OWNER: RunArgOwner = RunArgOwner::new("runtime.libkrun.cpu");
pub(crate) const LIBKRUN_CPUS_ANNOTATION_PREFIX: &str = "krun.cpus=";

pub(crate) fn append_cpu_annotation(run: &mut RunSpec, cpu_count: Option<u32>) {
    if let Some(cpu_count) = cpu_count {
        run.option(
            CPU_OWNER,
            "--annotation",
            format!("{}{}", LIBKRUN_CPUS_ANNOTATION_PREFIX, cpu_count),
        );
    }
}
use std::num::NonZero;

const PASS_ALL_CPUS_THRESHOLD: u32 = 6;
const HOST_CPU_RESERVATION: u32 = 2;

pub(crate) fn resolve_libkrun_cpu_count() -> Result<Option<u32>> {
    resolve_libkrun_cpu_count_for_host()
}

#[cfg(target_os = "linux")]
fn resolve_libkrun_cpu_count_for_host() -> Result<Option<u32>> {
    let available = std::thread::available_parallelism()
        .context("failed to detect available CPUs for libkrun krun.cpus default")?;
    resolve_libkrun_cpu_count_from_available(available).map(Some)
}

#[cfg(not(target_os = "linux"))]
fn resolve_libkrun_cpu_count_for_host() -> Result<Option<u32>> {
    Ok(None)
}

fn resolve_libkrun_cpu_count_from_available(available: NonZero<usize>) -> Result<u32> {
    let available = u32::try_from(available.get())
        .context("host available CPU count is too large for krun.cpus")?;
    if available <= PASS_ALL_CPUS_THRESHOLD {
        Ok(available)
    } else {
        Ok(available - HOST_CPU_RESERVATION)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn available(count: usize) -> NonZero<usize> {
        NonZero::new(count).expect("test CPU count should be non-zero")
    }

    #[test]
    fn libkrun_passes_all_available_cpus_up_to_threshold() {
        for count in [1, 2, 6] {
            assert_eq!(
                resolve_libkrun_cpu_count_from_available(available(count))
                    .expect("libkrun CPU policy should resolve"),
                count as u32,
                "{count} available CPUs should pass all CPUs",
            );
        }
    }

    #[test]
    fn libkrun_reserves_two_host_cpus_above_threshold() {
        for (count, expected) in [(7, 5), (8, 6), (9, 7), (10, 8), (16, 14), (32, 30)] {
            assert_eq!(
                resolve_libkrun_cpu_count_from_available(available(count))
                    .expect("libkrun CPU policy should resolve"),
                expected,
                "{count} available CPUs should map to {expected}",
            );
        }
    }

    #[cfg(target_pointer_width = "64")]
    #[test]
    fn libkrun_cpu_count_rejects_u32_overflow() {
        let too_many = (u32::MAX as usize) + 1;
        let err = resolve_libkrun_cpu_count_from_available(available(too_many))
            .expect_err("CPU counts above u32::MAX should fail");

        assert!(err.to_string().contains("too large for krun.cpus"));
    }
}
