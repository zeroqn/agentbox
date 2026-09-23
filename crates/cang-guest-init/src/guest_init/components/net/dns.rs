use anyhow::{Context, Result};
use std::path::Path;

const PASST_DNS_LINE: &str = "nameserver 169.254.1.1";

pub(in crate::guest_init) fn normalize_resolv_conf(input: Option<&str>) -> String {
    let mut out = String::from(PASST_DNS_LINE);
    out.push('\n');
    if let Some(input) = input {
        for line in input.lines() {
            if line != PASST_DNS_LINE {
                out.push_str(line);
                out.push('\n');
            }
        }
    }
    out
}

pub(in crate::guest_init) fn ensure_passt_resolv_conf(path: &Path) -> Result<()> {
    let current = match std::fs::read_to_string(path) {
        Ok(text) => Some(text),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
        Err(err) => return Err(err).with_context(|| format!("failed to read {}", path.display())),
    };
    crate::guest_init::fs::write_file(path, &normalize_resolv_conf(current.as_deref()), 0o644)
}

#[cfg(test)]
#[path = "dns_tests.rs"]
mod tests;
