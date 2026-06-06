//! CPU and memory contributions to the serialized launch config.

use anyhow::{Context, Result, anyhow};
use std::fs;

use super::super::model::{BYTES_PER_GIB, HOST_MEMINFO, KIB, MAX_GIB_FOR_KRUN_RAM_MIB};

pub(crate) fn resolve_cpu_count() -> Result<u8> {
    let available = std::thread::available_parallelism()
        .context("failed to detect available CPUs for loftd default")?
        .get();
    let count = if available <= 6 {
        available
    } else {
        available - 2
    };
    u8::try_from(count).context("host available CPU count is too large for libkrun vcpu config")
}

pub(crate) fn resolve_ram_mib(mem_gib: Option<u32>) -> Result<u32> {
    match mem_gib {
        Some(gib) => mem_gib_to_mib(gib),
        None => {
            let meminfo = fs::read_to_string(HOST_MEMINFO)
                .with_context(|| format!("failed to read host memory from {HOST_MEMINFO}"))?;
            default_ram_mib_from_meminfo(&meminfo)
        }
    }
}

pub(crate) fn default_ram_mib_from_meminfo(meminfo: &str) -> Result<u32> {
    let host_bytes = parse_meminfo_total_bytes(meminfo)?;
    let default_gib = default_mem_gib_from_host_bytes(host_bytes)?;
    mem_gib_to_mib(default_gib)
}

pub(crate) fn mem_gib_to_mib(gib: u32) -> Result<u32> {
    validate_mem_gib(gib)?;
    gib.checked_mul(1024)
        .ok_or_else(|| anyhow!("loftd --mem is too large for libkrun ram_mib"))
}

pub(crate) fn validate_mem_gib(gib: u32) -> Result<()> {
    if gib == 0 {
        anyhow::bail!("loftd --mem must be at least 1 GiB");
    }
    if gib > MAX_GIB_FOR_KRUN_RAM_MIB {
        anyhow::bail!("loftd --mem must be at most {MAX_GIB_FOR_KRUN_RAM_MIB} GiB");
    }
    Ok(())
}

pub(crate) fn default_mem_gib_from_host_bytes(host_bytes: u64) -> Result<u32> {
    let eighty_percent_bytes = (host_bytes / 5)
        .saturating_mul(4)
        .saturating_add((host_bytes % 5).saturating_mul(4) / 5);
    let default_gib = eighty_percent_bytes / BYTES_PER_GIB;

    if default_gib == 0 {
        anyhow::bail!("host memory is too small to derive a loftd --mem default of at least 1 GiB");
    }

    let default_gib = u32::try_from(default_gib)
        .context("host memory is too large to fit loftd --mem default")?;
    validate_mem_gib(default_gib)?;
    Ok(default_gib)
}

pub(crate) fn parse_meminfo_total_bytes(meminfo: &str) -> Result<u64> {
    let mem_total_line = meminfo
        .lines()
        .find(|line| line.starts_with("MemTotal:"))
        .ok_or_else(|| {
            anyhow!("host memory detection failed: MemTotal missing from {HOST_MEMINFO}")
        })?;

    let mut fields = mem_total_line.split_whitespace();
    let _label = fields.next();
    let value = fields
        .next()
        .ok_or_else(|| anyhow!("host memory detection failed: MemTotal value missing"))?;
    let unit = fields
        .next()
        .ok_or_else(|| anyhow!("host memory detection failed: MemTotal unit missing"))?;

    if unit != "kB" {
        anyhow::bail!("host memory detection failed: expected MemTotal in kB, got {unit}");
    }

    let kib = value
        .parse::<u64>()
        .with_context(|| format!("host memory detection failed: invalid MemTotal value {value}"))?;
    kib.checked_mul(KIB)
        .ok_or_else(|| anyhow!("host memory detection failed: MemTotal overflows bytes"))
}
