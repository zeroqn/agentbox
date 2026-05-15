use anyhow::{Context, Result, bail};
use std::path::Path;

use crate::guest_init::fs;

pub(in crate::guest_init) const GRAPHENE_HARDENED_MALLOC_LIB_ENV: &str =
    "AGENTBOX_GRAPHENE_HARDENED_MALLOC_LIB";
const LD_NIX_SO_PRELOAD_PATH: &str = "/etc/ld-nix.so.preload";

/// Owns runtime materialization of the Nix glibc allocator preload file.
pub(in crate::guest_init) fn ensure_from_env_if_root(is_root: bool) -> Result<()> {
    if !is_root {
        return Ok(());
    }
    let Ok(allocator_lib) = std::env::var(GRAPHENE_HARDENED_MALLOC_LIB_ENV) else {
        return Ok(());
    };
    ensure_at(Path::new(LD_NIX_SO_PRELOAD_PATH), &allocator_lib)
}

fn ensure_at(path: &Path, allocator_lib: &str) -> Result<()> {
    validate_allocator_lib(allocator_lib)?;
    fs::write_file(path, &preload_contents(allocator_lib), 0o644).with_context(|| {
        format!(
            "failed to materialize Nix allocator preload at {}",
            path.display()
        )
    })
}

fn preload_contents(allocator_lib: &str) -> String {
    format!("{allocator_lib}\n")
}

fn validate_allocator_lib(allocator_lib: &str) -> Result<()> {
    if allocator_lib.is_empty() {
        bail!("{GRAPHENE_HARDENED_MALLOC_LIB_ENV} must not be empty");
    }
    if !Path::new(allocator_lib).is_absolute() {
        bail!("{GRAPHENE_HARDENED_MALLOC_LIB_ENV} must be an absolute path");
    }
    if !allocator_lib.ends_with("/lib/libhardened_malloc.so") {
        bail!("{GRAPHENE_HARDENED_MALLOC_LIB_ENV} must point at libhardened_malloc.so");
    }
    Ok(())
}

#[cfg(test)]
#[path = "allocator_tests.rs"]
mod tests;
