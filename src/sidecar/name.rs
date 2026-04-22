use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::{derive_workspace_slug, SIDECAR_NAME_PREFIX};

pub fn allocate_generation_id(cwd: &Path, image_id: &str) -> String {
    let mut hasher = DefaultHasher::new();
    cwd.hash(&mut hasher);
    image_id.hash(&mut hasher);
    process::id().hash(&mut hasher);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    now.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

pub fn derive_sidecar_name(cwd: &Path, image_id: &str, generation: &str) -> String {
    let workspace_slug = derive_workspace_slug(cwd);
    let mut hasher = DefaultHasher::new();
    cwd.hash(&mut hasher);
    image_id.hash(&mut hasher);
    generation.hash(&mut hasher);
    let digest = hasher.finish();
    format!("{SIDECAR_NAME_PREFIX}-{workspace_slug}-{digest:016x}")
}

pub fn derive_legacy_generation_id(sidecar_name: &str) -> String {
    let mut slug = String::new();
    let mut last_was_separator = false;

    for ch in sidecar_name.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            last_was_separator = false;
        } else if !slug.is_empty() && !last_was_separator {
            slug.push('-');
            last_was_separator = true;
        }
    }

    let slug = slug.trim_matches('-');
    if slug.is_empty() {
        "legacy-workspace".to_owned()
    } else {
        format!("legacy-{slug}")
    }
}
