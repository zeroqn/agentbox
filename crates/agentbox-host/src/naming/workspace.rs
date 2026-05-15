use std::path::Path;

const WORKSPACE_SLUG_FALLBACK: &str = "workspace";
const WORKSPACE_SLUG_MAX_LEN: usize = 32;

pub(crate) fn derive_workspace_slug(cwd: &Path) -> String {
    let workspace_name = cwd
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or(WORKSPACE_SLUG_FALLBACK);

    let mut slug = String::new();
    let mut last_was_separator = false;

    for ch in workspace_name.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            last_was_separator = false;
        } else if !slug.is_empty() && !last_was_separator {
            slug.push('-');
            last_was_separator = true;
        }
    }

    let truncated = slug
        .trim_matches('-')
        .chars()
        .take(WORKSPACE_SLUG_MAX_LEN)
        .collect::<String>();
    let trimmed = truncated.trim_matches('-');

    if trimmed.is_empty() {
        WORKSPACE_SLUG_FALLBACK.to_owned()
    } else {
        trimmed.to_owned()
    }
}
