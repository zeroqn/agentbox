use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::Path;

use crate::{derive_workspace_slug, SIDECAR_NAME_PREFIX};

pub fn derive_sidecar_name(cwd: &Path, image_id: &str) -> String {
    let workspace_slug = derive_workspace_slug(cwd);
    let mut hasher = DefaultHasher::new();
    cwd.hash(&mut hasher);
    image_id.hash(&mut hasher);
    let digest = hasher.finish();
    format!("{SIDECAR_NAME_PREFIX}-{workspace_slug}-{digest:016x}")
}
