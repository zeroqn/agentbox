use anyhow::{Context, Result, anyhow, bail};
use std::collections::BTreeMap;
use std::path::Path;

use crate::guest_init::fs;

pub(in crate::guest_init) const MIMALLOC_LIB_ENV: &str = "LOFTD_MIMALLOC_LIB";
pub(in crate::guest_init) const GRAPHENE_HARDENED_MALLOC_LIB_ENV: &str =
    "LOFTD_GRAPHENE_HARDENED_MALLOC_LIB";
pub(in crate::guest_init) const NIX_ALLOCATOR_ENV: &str = "LOFTD_NIX_ALLOCATOR";
const ALLOCATOR_METADATA_PATH: &str = "/etc/nix-allocator-libs";
const LD_NIX_SO_PRELOAD_PATH: &str = "/etc/ld-nix.so.preload";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AllocatorKind {
    Mimalloc,
    Hardened,
    Glibc,
}

impl AllocatorKind {
    fn metadata_key(self) -> &'static str {
        match self {
            Self::Mimalloc => "mimalloc",
            Self::Hardened => "hardened",
            Self::Glibc => unreachable!("glibc does not use allocator metadata"),
        }
    }

    fn env_name(self) -> &'static str {
        match self {
            Self::Mimalloc => MIMALLOC_LIB_ENV,
            Self::Hardened => GRAPHENE_HARDENED_MALLOC_LIB_ENV,
            Self::Glibc => {
                unreachable!("glibc does not use an allocator library environment variable")
            }
        }
    }

    fn library_suffix(self) -> &'static str {
        match self {
            Self::Mimalloc => "/lib/libmimalloc.so",
            Self::Hardened => "/lib/libhardened_malloc.so",
            Self::Glibc => unreachable!("glibc does not use an allocator library suffix"),
        }
    }

    fn display_name(self) -> &'static str {
        match self {
            Self::Mimalloc => "mimalloc",
            Self::Hardened => "hardened_malloc",
            Self::Glibc => unreachable!("glibc does not use an allocator library display name"),
        }
    }
}

/// Owns runtime materialization of the Nix glibc allocator preload file.
pub(in crate::guest_init) fn ensure_from_env_if_root(is_root: bool) -> Result<()> {
    if !is_root {
        return Ok(());
    }

    let kind = allocator_kind_from_env()?;
    if kind == AllocatorKind::Glibc {
        return disable_at(Path::new(LD_NIX_SO_PRELOAD_PATH));
    }

    let metadata = std::fs::read_to_string(ALLOCATOR_METADATA_PATH).ok();
    let env_fallback = std::env::var(kind.env_name()).ok();
    let Some(allocator_lib) =
        select_allocator_lib(kind, metadata.as_deref(), env_fallback.as_deref())?
    else {
        return Ok(());
    };

    ensure_at(Path::new(LD_NIX_SO_PRELOAD_PATH), &allocator_lib, kind)
}

fn allocator_kind_from_env() -> Result<AllocatorKind> {
    match std::env::var(NIX_ALLOCATOR_ENV) {
        Ok(value) => parse_allocator_kind(&value),
        Err(std::env::VarError::NotPresent) => Ok(AllocatorKind::Mimalloc),
        Err(std::env::VarError::NotUnicode(_)) => bail!("LOFTD_NIX_ALLOCATOR must be valid UTF-8"),
    }
}

fn parse_allocator_kind(value: &str) -> Result<AllocatorKind> {
    match value {
        "" | "mimalloc" => Ok(AllocatorKind::Mimalloc),
        "hardened" => Ok(AllocatorKind::Hardened),
        "glibc" => Ok(AllocatorKind::Glibc),
        _ => bail!("LOFTD_NIX_ALLOCATOR must be 'mimalloc', 'hardened', or 'glibc'"),
    }
}

fn select_allocator_lib(
    kind: AllocatorKind,
    metadata: Option<&str>,
    env_fallback: Option<&str>,
) -> Result<Option<String>> {
    if let Some(metadata) = metadata {
        let entries = parse_allocator_metadata(metadata)?;
        if let Some(value) = entries.get(kind.metadata_key()) {
            validate_allocator_lib(value, kind)?;
            return Ok(Some(value.to_owned()));
        }
    }

    if let Some(value) = env_fallback.filter(|value| !value.is_empty()) {
        validate_allocator_lib(value, kind)?;
        return Ok(Some(value.to_owned()));
    }

    if kind == AllocatorKind::Hardened {
        bail!(
            "LOFTD_NIX_ALLOCATOR=hardened requires {} in {ALLOCATOR_METADATA_PATH} or LOFTD_GRAPHENE_HARDENED_MALLOC_LIB",
            kind.metadata_key()
        );
    }

    Ok(None)
}

fn parse_allocator_metadata(metadata: &str) -> Result<BTreeMap<String, String>> {
    let mut entries = BTreeMap::new();
    for (line_index, line) in metadata.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| anyhow!("allocator metadata line {} is missing '='", line_index + 1))?;
        let key = key.trim();
        let value = value.trim();
        if key.is_empty() || value.is_empty() {
            bail!(
                "allocator metadata line {} must contain key and value",
                line_index + 1
            );
        }
        entries.insert(key.to_owned(), value.to_owned());
    }
    Ok(entries)
}

fn disable_at(path: &Path) -> Result<()> {
    fs::write_file(path, "", 0o644).with_context(|| {
        format!(
            "failed to disable Nix allocator preload at {}",
            path.display()
        )
    })
}

fn ensure_at(path: &Path, allocator_lib: &str, kind: AllocatorKind) -> Result<()> {
    validate_allocator_lib(allocator_lib, kind)?;
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

fn validate_allocator_lib(allocator_lib: &str, kind: AllocatorKind) -> Result<()> {
    if allocator_lib.is_empty() {
        bail!("{} must not be empty", kind.env_name());
    }
    if !Path::new(allocator_lib).is_absolute() {
        bail!("{} must be an absolute path", kind.env_name());
    }
    if !allocator_lib.ends_with(kind.library_suffix()) {
        bail!("{} must point at {}", kind.env_name(), kind.display_name());
    }
    Ok(())
}

#[cfg(test)]
#[path = "allocator_tests.rs"]
mod tests;
