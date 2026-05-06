use anyhow::{Context, Result};
use std::num::NonZero;

use crate::TaskContainerMode;

const LIBKRUN_CPU_THRESHOLD: usize = 8;
const GENERIC_LIBKRUN_CPU_CAP: u32 = 16;
const HOST_CPU_RESERVATION: u32 = 2;

pub(crate) fn resolve_libkrun_cpu_count(task_mode: TaskContainerMode) -> Result<Option<u32>> {
    if task_mode != TaskContainerMode::Libkrun {
        return Ok(None);
    }

    resolve_libkrun_cpu_count_for_host(task_mode)
}

#[cfg(target_os = "linux")]
fn resolve_libkrun_cpu_count_for_host(task_mode: TaskContainerMode) -> Result<Option<u32>> {
    let available = std::thread::available_parallelism()
        .context("failed to detect available CPUs for libkrun krun.cpus default")?;
    resolve_libkrun_cpu_count_from_available(task_mode, available, GENERIC_LIBKRUN_CPU_CAP)
}

#[cfg(not(target_os = "linux"))]
fn resolve_libkrun_cpu_count_for_host(_task_mode: TaskContainerMode) -> Result<Option<u32>> {
    Ok(None)
}

fn resolve_libkrun_cpu_count_from_available(
    task_mode: TaskContainerMode,
    available: NonZero<usize>,
    cap: u32,
) -> Result<Option<u32>> {
    if task_mode != TaskContainerMode::Libkrun {
        return Ok(None);
    }

    if cap == 0 {
        anyhow::bail!("libkrun CPU cap must be at least 1");
    }

    let available = available.get();
    if available <= LIBKRUN_CPU_THRESHOLD {
        return Ok(None);
    }

    let available =
        u32::try_from(available).context("host available CPU count is too large for krun.cpus")?;
    Ok(Some(
        available.saturating_sub(HOST_CPU_RESERVATION).min(cap),
    ))
}

#[cfg(test)]
mod tests {
    use crate::cpu::{resolve_libkrun_cpu_count_from_available, GENERIC_LIBKRUN_CPU_CAP};
    use crate::TaskContainerMode;
    use std::num::NonZero;

    fn available(count: usize) -> NonZero<usize> {
        NonZero::new(count).expect("test CPU count should be non-zero")
    }

    #[test]
    fn native_mode_omits_cpu_count() {
        assert_eq!(
            resolve_libkrun_cpu_count_from_available(
                TaskContainerMode::Native,
                available(32),
                GENERIC_LIBKRUN_CPU_CAP,
            )
            .expect("native mode should resolve"),
            None
        );
    }

    #[test]
    fn libkrun_default_cap_omits_cpu_count_up_to_threshold() {
        for count in [1, 2, 8] {
            assert_eq!(
                resolve_libkrun_cpu_count_from_available(
                    TaskContainerMode::Libkrun,
                    available(count),
                    GENERIC_LIBKRUN_CPU_CAP,
                )
                .expect("libkrun CPU policy should resolve"),
                None,
                "{count} available CPUs should omit krun.cpus",
            );
        }
    }

    #[test]
    fn libkrun_default_cap_reserves_host_cpus_and_caps_result() {
        for (count, expected) in [
            (9, Some(7)),
            (10, Some(8)),
            (16, Some(14)),
            (17, Some(15)),
            (18, Some(16)),
            (32, Some(16)),
        ] {
            assert_eq!(
                resolve_libkrun_cpu_count_from_available(
                    TaskContainerMode::Libkrun,
                    available(count),
                    GENERIC_LIBKRUN_CPU_CAP,
                )
                .expect("libkrun CPU policy should resolve"),
                expected,
                "{count} available CPUs should map to {expected:?}",
            );
        }
    }

    #[test]
    fn libkrun_cpu_count_supports_injected_lower_caps() {
        assert_eq!(
            resolve_libkrun_cpu_count_from_available(TaskContainerMode::Libkrun, available(16), 8)
                .expect("cap 8 should resolve"),
            Some(8)
        );
        assert_eq!(
            resolve_libkrun_cpu_count_from_available(TaskContainerMode::Libkrun, available(9), 4)
                .expect("cap 4 should resolve"),
            Some(4)
        );
    }

    #[test]
    fn libkrun_cpu_count_rejects_zero_cap() {
        let err =
            resolve_libkrun_cpu_count_from_available(TaskContainerMode::Libkrun, available(9), 0)
                .expect_err("zero cap should be rejected");

        assert!(err.to_string().contains("CPU cap"));
    }

    #[cfg(target_pointer_width = "64")]
    #[test]
    fn libkrun_cpu_count_rejects_u32_overflow() {
        let too_many = (u32::MAX as usize) + 1;
        let err = resolve_libkrun_cpu_count_from_available(
            TaskContainerMode::Libkrun,
            available(too_many),
            GENERIC_LIBKRUN_CPU_CAP,
        )
        .expect_err("CPU counts above u32::MAX should fail");

        assert!(err.to_string().contains("too large for krun.cpus"));
    }
}
