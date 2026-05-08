use anyhow::Result;

const MIB_PER_GIB: u32 = 1024;
const MAX_GIB_FOR_KRUN_RAM_MIB: u32 = u32::MAX / MIB_PER_GIB;

pub(crate) fn parse_mem_gib_arg(value: &str) -> std::result::Result<u32, String> {
    let gib = value
        .parse::<u32>()
        .map_err(|_| "must be a positive integer number of GiB".to_owned())?;

    validate_mem_gib(gib).map_err(|err| err.to_string())?;
    Ok(gib)
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
}
