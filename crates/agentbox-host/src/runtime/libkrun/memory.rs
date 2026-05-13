use anyhow::{anyhow, Context, Result};
use std::fs;

const KIB: u64 = 1024;
const MIB_PER_GIB: u32 = 1024;
const BYTES_PER_GIB: u64 = 1024 * 1024 * 1024;
const MAX_GIB_FOR_KRUN_RAM_MIB: u32 = u32::MAX / MIB_PER_GIB;
const HOST_MEMINFO: &str = "/proc/meminfo";

pub(crate) fn parse_mem_gib_arg(value: &str) -> std::result::Result<u32, String> {
    let gib = value
        .parse::<u32>()
        .map_err(|_| "must be a positive integer number of GiB".to_owned())?;

    validate_mem_gib(gib).map_err(|err| err.to_string())?;
    Ok(gib)
}

pub(crate) fn resolve_libkrun_ram_mib(explicit_mem_gib: Option<u32>) -> Result<u32> {
    match explicit_mem_gib {
        Some(gib) => mem_gib_to_mib(gib),
        None => {
            let meminfo = fs::read_to_string(HOST_MEMINFO)
                .with_context(|| format!("failed to read host memory from {HOST_MEMINFO}"))?;
            default_libkrun_ram_mib_from_meminfo(&meminfo)
        }
    }
}

fn default_libkrun_ram_mib_from_meminfo(meminfo: &str) -> Result<u32> {
    let host_bytes = parse_meminfo_total_bytes(meminfo)?;
    let default_gib = default_libkrun_mem_gib_from_host_bytes(host_bytes)?;
    mem_gib_to_mib(default_gib)
}

fn validate_mem_gib(gib: u32) -> Result<()> {
    if gib == 0 {
        anyhow::bail!("must be at least 1 GiB");
    }

    if gib > MAX_GIB_FOR_KRUN_RAM_MIB {
        anyhow::bail!("must be at most {MAX_GIB_FOR_KRUN_RAM_MIB} GiB");
    }

    Ok(())
}

fn mem_gib_to_mib(gib: u32) -> Result<u32> {
    validate_mem_gib(gib)?;
    gib.checked_mul(MIB_PER_GIB)
        .ok_or_else(|| anyhow!("memory size is too large for krun.ram_mib"))
}

fn default_libkrun_mem_gib_from_host_bytes(host_bytes: u64) -> Result<u32> {
    let eighty_percent_bytes = (host_bytes / 5)
        .saturating_mul(4)
        .saturating_add((host_bytes % 5).saturating_mul(4) / 5);
    let default_gib = eighty_percent_bytes / BYTES_PER_GIB;

    if default_gib == 0 {
        anyhow::bail!(
            "host memory is too small to derive a libkrun --mem default of at least 1 GiB"
        );
    }

    let default_gib = u32::try_from(default_gib)
        .context("host memory is too large to fit libkrun --mem default")?;
    validate_mem_gib(default_gib)?;
    Ok(default_gib)
}

fn parse_meminfo_total_bytes(meminfo: &str) -> Result<u64> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_mem_gib_arg_accepts_positive_integer_gib() {
        assert_eq!(parse_mem_gib_arg("8").expect("8 GiB should parse"), 8);
    }

    #[test]
    fn parse_mem_gib_arg_rejects_zero() {
        assert!(parse_mem_gib_arg("0").is_err());
    }

    #[test]
    fn parse_mem_gib_arg_rejects_suffixes_and_decimals() {
        assert!(parse_mem_gib_arg("8g").is_err());
        assert!(parse_mem_gib_arg("1.5").is_err());
    }

    #[test]
    fn mem_gib_to_mib_converts_for_krun_annotation() {
        assert_eq!(mem_gib_to_mib(8).expect("8 GiB should convert"), 8192);
    }

    #[test]
    fn default_libkrun_mem_gib_floors_eighty_percent_to_whole_gib() {
        assert_eq!(
            default_libkrun_mem_gib_from_host_bytes(10 * BYTES_PER_GIB)
                .expect("10 GiB host should produce default"),
            8
        );
        assert_eq!(
            default_libkrun_mem_gib_from_host_bytes(2 * BYTES_PER_GIB)
                .expect("2 GiB host should produce default"),
            1
        );
    }

    #[test]
    fn default_libkrun_mem_gib_rejects_unusable_small_host_memory() {
        assert!(default_libkrun_mem_gib_from_host_bytes(BYTES_PER_GIB).is_err());
    }

    #[test]
    fn parse_meminfo_total_bytes_reads_memtotal_kib() {
        let meminfo = "MemTotal:       10485760 kB\nMemFree:         1024 kB\n";
        assert_eq!(
            parse_meminfo_total_bytes(meminfo).expect("MemTotal should parse"),
            10 * BYTES_PER_GIB
        );
    }

    #[test]
    fn parse_meminfo_total_bytes_rejects_missing_memtotal() {
        assert!(parse_meminfo_total_bytes("MemFree: 1024 kB\n").is_err());
    }

    #[test]
    fn default_libkrun_ram_mib_from_meminfo_defaults_to_eighty_percent() {
        let meminfo = "MemTotal:       10485760 kB\n";
        assert_eq!(
            default_libkrun_ram_mib_from_meminfo(meminfo).expect("libkrun default should resolve"),
            8192
        );
    }

    #[test]
    fn resolve_libkrun_ram_mib_uses_explicit_value() {
        assert_eq!(
            resolve_libkrun_ram_mib(Some(4)).expect("explicit libkrun memory should resolve"),
            4096
        );
    }
}
