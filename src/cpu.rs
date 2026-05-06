use anyhow::{Context, Result};
use std::num::NonZero;

use crate::ContainerRuntimeMode;

const PASS_ALL_CPUS_THRESHOLD: u32 = 6;
const HOST_CPU_RESERVATION: u32 = 2;

pub(crate) fn resolve_libkrun_cpu_count(runtime_mode: ContainerRuntimeMode) -> Result<Option<u32>> {
    if runtime_mode != ContainerRuntimeMode::Libkrun {
        return Ok(None);
    }

    resolve_libkrun_cpu_count_for_host(runtime_mode)
}

#[cfg(target_os = "linux")]
fn resolve_libkrun_cpu_count_for_host(runtime_mode: ContainerRuntimeMode) -> Result<Option<u32>> {
    let available = std::thread::available_parallelism()
        .context("failed to detect available CPUs for libkrun krun.cpus default")?;
    resolve_libkrun_cpu_count_from_available(runtime_mode, available)
}

#[cfg(not(target_os = "linux"))]
fn resolve_libkrun_cpu_count_for_host(_runtime_mode: ContainerRuntimeMode) -> Result<Option<u32>> {
    Ok(None)
}

fn resolve_libkrun_cpu_count_from_available(
    runtime_mode: ContainerRuntimeMode,
    available: NonZero<usize>,
) -> Result<Option<u32>> {
    if runtime_mode != ContainerRuntimeMode::Libkrun {
        return Ok(None);
    }

    let available = u32::try_from(available.get())
        .context("host available CPU count is too large for krun.cpus")?;
    if available <= PASS_ALL_CPUS_THRESHOLD {
        Ok(Some(available))
    } else {
        Ok(Some(available - HOST_CPU_RESERVATION))
    }
}

#[cfg(test)]
mod tests {
    use crate::cpu::resolve_libkrun_cpu_count_from_available;
    use crate::ContainerRuntimeMode;
    use std::num::NonZero;

    fn available(count: usize) -> NonZero<usize> {
        NonZero::new(count).expect("test CPU count should be non-zero")
    }

    #[test]
    fn native_mode_omits_cpu_count() {
        assert_eq!(
            resolve_libkrun_cpu_count_from_available(ContainerRuntimeMode::Native, available(32))
                .expect("native mode should resolve"),
            None
        );
    }

    #[test]
    fn libkrun_passes_all_available_cpus_up_to_threshold() {
        for count in [1, 2, 6] {
            assert_eq!(
                resolve_libkrun_cpu_count_from_available(
                    ContainerRuntimeMode::Libkrun,
                    available(count),
                )
                .expect("libkrun CPU policy should resolve"),
                Some(count as u32),
                "{count} available CPUs should pass all CPUs",
            );
        }
    }

    #[test]
    fn libkrun_reserves_two_host_cpus_above_threshold() {
        for (count, expected) in [
            (7, Some(5)),
            (8, Some(6)),
            (9, Some(7)),
            (10, Some(8)),
            (16, Some(14)),
            (18, Some(16)),
            (32, Some(30)),
        ] {
            assert_eq!(
                resolve_libkrun_cpu_count_from_available(
                    ContainerRuntimeMode::Libkrun,
                    available(count),
                )
                .expect("libkrun CPU policy should resolve"),
                expected,
                "{count} available CPUs should map to {expected:?}",
            );
        }
    }

    #[cfg(target_pointer_width = "64")]
    #[test]
    fn libkrun_cpu_count_rejects_u32_overflow() {
        let too_many = (u32::MAX as usize) + 1;
        let err = resolve_libkrun_cpu_count_from_available(
            ContainerRuntimeMode::Libkrun,
            available(too_many),
        )
        .expect_err("CPU counts above u32::MAX should fail");

        assert!(err.to_string().contains("too large for krun.cpus"));
    }
}
