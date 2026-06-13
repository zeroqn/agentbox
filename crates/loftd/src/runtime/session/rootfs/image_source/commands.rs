use anyhow::{Context, Result, anyhow, bail};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::runtime::launch::plan::ImageSelection;
use crate::runtime::session::rootfs::task::BtrfsRootfsCommands;

use super::{
    BTRFS_IMAGE_CACHE_DIR, BtrfsImageCacheEntry, BuildahCommands, CACHE_METADATA_FILE,
    CACHE_ROOTFS_DIR, ImageSourceCacheStatus, inspect_image_digest,
    materialize_btrfs_source_rootfs, parse_cache_metadata, remove_cache_entry_tree,
    reset_cache_entry, safe_digest_key,
};

const STAGING_DIR: &str = ".staging";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ImageCacheCommand {
    Sync { reference: String },
    List,
    Remove { target: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ImageCacheCommandOutput {
    Sync(ImageSyncReport),
    List(Vec<ImageCacheListEntry>),
    Remove(ImageRemoveReport),
}

impl ImageCacheCommandOutput {
    pub(crate) fn render_stdout(&self) -> String {
        match self {
            Self::Sync(report) => report.render_stdout(),
            Self::List(entries) => render_list_stdout(entries),
            Self::Remove(report) => report.render_stdout(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ImageSyncReport {
    pub(crate) reference: String,
    pub(crate) digest: Option<String>,
    pub(crate) digest_key: Option<String>,
    pub(crate) cache_status: ImageSourceCacheStatus,
    pub(crate) cache_path: Option<PathBuf>,
}

impl ImageSyncReport {
    fn render_stdout(&self) -> String {
        let digest = self.digest.as_deref().unwrap_or("<unknown>");
        let digest_key = self.digest_key.as_deref().unwrap_or("<none>");
        let cache_path = self
            .cache_path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "<none>".to_owned());
        format!(
            "synced\t{}\t{}\t{}\t{}\t{}\n",
            self.reference,
            digest,
            digest_key,
            self.cache_status.as_profile_value(),
            cache_path
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ImageCacheListEntry {
    pub(crate) digest_key: Option<String>,
    pub(crate) digest: Option<String>,
    pub(crate) selected_reference: Option<String>,
    pub(crate) repository: String,
    pub(crate) tag: String,
    pub(crate) image_id: Option<String>,
    pub(crate) status: ImageCacheEntryStatus,
    pub(crate) buildah_status: BuildahMatchStatus,
    pub(crate) entry_dir: Option<PathBuf>,
    pub(crate) short_path: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ImageCacheEntryStatus {
    Complete,
    Incomplete,
    Invalid,
}

impl ImageCacheEntryStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Complete => "complete",
            Self::Incomplete => "incomplete",
            Self::Invalid => "invalid",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BuildahMatchStatus {
    Match,
    MissingOrDigestless,
    DigestMismatch,
    NoReference,
    NoCacheDigest,
    InvalidCache,
}

impl BuildahMatchStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Match => "match",
            Self::MissingOrDigestless => "missing-or-digestless",
            Self::DigestMismatch => "digest-mismatch",
            Self::NoReference => "no-reference",
            Self::NoCacheDigest => "no-cache-digest",
            Self::InvalidCache => "invalid-cache",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ImageRemoveReport {
    pub(crate) target: String,
    pub(crate) digest_key: String,
    pub(crate) digest: String,
    pub(crate) local_image_removal: LocalImageRemoval,
}

impl ImageRemoveReport {
    fn render_stdout(&self) -> String {
        format!(
            "removed-cache\t{}\t{}\t{}\n{}\n",
            self.target,
            self.digest_key,
            self.digest,
            self.local_image_removal.as_stdout_line()
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LocalImageRemoval {
    Removed { reference: String },
    Skipped { reason: String },
}

impl LocalImageRemoval {
    fn as_stdout_line(&self) -> String {
        match self {
            Self::Removed { reference } => format!("removed-local\t{reference}"),
            Self::Skipped { reason } => format!("skipped-local\t{reason}"),
        }
    }
}

pub(crate) fn run_image_cache_command(
    command: ImageCacheCommand,
    image_cache_root: &Path,
    buildah: &impl BuildahCommands,
    btrfs: &impl BtrfsRootfsCommands,
) -> Result<ImageCacheCommandOutput> {
    match command {
        ImageCacheCommand::Sync { reference } => {
            sync_image_cache(&reference, image_cache_root, buildah, btrfs)
                .map(ImageCacheCommandOutput::Sync)
        }
        ImageCacheCommand::List => {
            list_image_cache(image_cache_root, buildah).map(ImageCacheCommandOutput::List)
        }
        ImageCacheCommand::Remove { target } => {
            remove_image_cache_entry(&target, image_cache_root, buildah, btrfs)
                .map(ImageCacheCommandOutput::Remove)
        }
    }
}

fn sync_image_cache(
    reference: &str,
    image_cache_root: &Path,
    buildah: &impl BuildahCommands,
    btrfs: &impl BtrfsRootfsCommands,
) -> Result<ImageSyncReport> {
    let reference = reference.trim();
    if reference.is_empty() {
        bail!("image reference must not be empty");
    }
    let reference = resolve_sync_reference(reference, image_cache_root, buildah)?;

    let staging = ImageCacheStagingDir::create(image_cache_root)?;
    let staging_rootfs = staging.path().join(CACHE_ROOTFS_DIR);
    let materialized = materialize_btrfs_source_rootfs(
        &ImageSelection::Explicit {
            reference: reference.to_owned(),
        },
        &staging_rootfs,
        image_cache_root,
        buildah,
        btrfs,
    );
    let cleanup = staging.cleanup(btrfs);

    let rootfs = match (materialized, cleanup) {
        (Ok(rootfs), Ok(())) => rootfs,
        (Ok(_), Err(cleanup_err)) => return Err(cleanup_err),
        (Err(err), Ok(())) => return Err(err),
        (Err(err), Err(cleanup_err)) => {
            return Err(cleanup_err.context(format!(
                "failed to clean loftd image cache staging after sync error: {err:#}"
            )));
        }
    };

    Ok(ImageSyncReport {
        reference: rootfs.selected_reference,
        digest: rootfs.image_digest,
        digest_key: rootfs.cache_profile.digest_key,
        cache_status: rootfs.cache_profile.status,
        cache_path: rootfs.cache_profile.cache_path,
    })
}

fn list_image_cache(
    image_cache_root: &Path,
    buildah: &impl BuildahCommands,
) -> Result<Vec<ImageCacheListEntry>> {
    let cache_dir = image_cache_root.join(BTRFS_IMAGE_CACHE_DIR);
    let buildah_rows = read_buildah_inventory(buildah);
    let mut entries = Vec::new();

    if cache_dir.exists() {
        for dir_entry in fs::read_dir(&cache_dir).with_context(|| {
            format!("failed to read loftd image cache '{}'", cache_dir.display())
        })? {
            let dir_entry = dir_entry?;
            let file_type = dir_entry.file_type()?;
            if !file_type.is_dir() {
                continue;
            }
            let digest_key = dir_entry.file_name().to_string_lossy().to_string();
            entries.push(read_list_entry(digest_key, dir_entry.path(), buildah));
        }
    }

    let original_image_ids: Vec<Option<String>> =
        entries.iter().map(|e| e.image_id.clone()).collect();
    let reconcile_rows = buildah_rows.clone();
    let mut result = enrich_list_entries(entries, buildah_rows);

    // Reconcile all entries against buildah by image_id prefix.
    // This runs AFTER enrichment so image_id-based TAGs override
    // any stale selected_reference matches from Pass 2.
    if !reconcile_rows.is_empty() {
        for (result_idx, entry) in result.iter_mut().enumerate() {
            // Use original image_id from before enrichment, not the
            // potentially-overwritten one from with_buildah_row.
            let lookup_id = original_image_ids
                .get(result_idx)
                .and_then(|id| id.as_deref());
            if let Some(image_id) = lookup_id
                && let Some(row) = reconcile_rows.iter().find(|row| {
                    row.image_id
                        .as_deref()
                        .is_some_and(|row_id| image_id.starts_with(row_id))
                })
            {
                entry.repository = row.repository.clone();
                entry.tag = row.tag.clone();
            }
        }
    }

    for entry in &mut result {
        entry.short_path = entry.entry_dir.as_ref().and_then(|entry_dir| {
            let stripped = entry_dir.strip_prefix(image_cache_root).ok()?;
            Some(format!("{}", stripped.display()))
        });
    }
    Ok(result)
}

fn read_list_entry(
    digest_key: String,
    entry_dir: PathBuf,
    buildah: &impl BuildahCommands,
) -> ImageCacheListEntry {
    let rootfs_path = entry_dir.join(CACHE_ROOTFS_DIR);
    let metadata_path = entry_dir.join(CACHE_METADATA_FILE);
    if !rootfs_path.is_dir() || !metadata_path.is_file() {
        return ImageCacheListEntry::cached(
            digest_key,
            digest_from_digest_key(&entry_dir),
            None,
            ImageCacheEntryStatus::Incomplete,
            BuildahMatchStatus::NoReference,
            entry_dir,
            None,
        );
    }

    match fs::read_to_string(&metadata_path)
        .with_context(|| format!("failed to read '{}'", metadata_path.display()))
        .and_then(|metadata| parse_cache_metadata(&metadata, &rootfs_path))
    {
        Ok(rootfs) => {
            let digest = rootfs.image_local_digest.or(rootfs.image_digest);
            let status = if digest
                .as_deref()
                .and_then(|digest| safe_digest_key(digest).ok())
                .as_deref()
                == Some(digest_key.as_str())
            {
                ImageCacheEntryStatus::Complete
            } else {
                ImageCacheEntryStatus::Invalid
            };
            let selected_reference = rootfs.selected_reference;
            let buildah_status = if status == ImageCacheEntryStatus::Complete {
                buildah_match_status(
                    Some(selected_reference.as_str()),
                    digest.as_deref(),
                    buildah,
                )
            } else {
                BuildahMatchStatus::InvalidCache
            };
            ImageCacheListEntry::cached(
                digest_key,
                digest,
                Some(selected_reference),
                status,
                buildah_status,
                entry_dir,
                rootfs.image_id,
            )
        }
        Err(_) => ImageCacheListEntry::cached(
            digest_key,
            digest_from_digest_key(&entry_dir),
            None,
            ImageCacheEntryStatus::Invalid,
            BuildahMatchStatus::InvalidCache,
            entry_dir,
            None,
        ),
    }
}

fn remove_image_cache_entry(
    target: &str,
    image_cache_root: &Path,
    buildah: &impl BuildahCommands,
    btrfs: &impl BtrfsRootfsCommands,
) -> Result<ImageRemoveReport> {
    let target = target.trim();
    if target.is_empty() {
        bail!("image cache remove target must not be empty");
    }

    let entry = resolve_remove_target(image_cache_root, target, buildah)?;
    if !entry.entry_dir.exists() {
        bail!(
            "loftd image cache entry '{}' does not exist",
            entry.entry_dir.display()
        );
    }
    let cached = read_cached_metadata_for_remove(&entry);
    let selected_reference = cached.as_ref().ok().and_then(|rootfs| {
        (rootfs.image_digest.as_deref() == Some(entry.digest.as_str()))
            .then(|| rootfs.selected_reference.clone())
    });
    reset_cache_entry(&entry, btrfs)?;

    let local_image_removal = match (selected_reference, entry.digest.as_str()) {
        (Some(reference), digest) => remove_guarded_local_image(&reference, digest, buildah)?,
        (None, _) => LocalImageRemoval::Skipped {
            reason: "cache metadata missing or invalid; no selected image reference to guard"
                .to_owned(),
        },
    };

    Ok(ImageRemoveReport {
        target: target.to_owned(),
        digest_key: entry.digest_key,
        digest: entry.digest,
        local_image_removal,
    })
}

fn resolve_remove_target(
    image_cache_root: &Path,
    target: &str,
    buildah: &impl BuildahCommands,
) -> Result<BtrfsImageCacheEntry> {
    if let Some(entry) = exact_digest_entry(image_cache_root, target)? {
        return Ok(entry);
    }

    match resolve_local_selector(image_cache_root, buildah, target)? {
        SelectorResolution::One(row) => {
            let digest_key = row.digest_key.as_deref().ok_or_else(|| {
                anyhow!("image selector '{target}' matched a row without a loftd cache key")
            })?;
            let digest = digest_from_digest_key_path_component(digest_key).ok_or_else(|| {
                anyhow!("image selector '{target}' matched a row with an invalid loftd cache key")
            })?;
            BtrfsImageCacheEntry::new(image_cache_root, &digest)
        }
        SelectorResolution::Multiple(rows) => bail!(
            "image selector '{target}' matched multiple rows; refusing to remove; candidates: {}",
            format_selector_candidates(&rows)
        ),
        SelectorResolution::None => {
            bail!("image selector '{target}' did not match a loftd image cache row")
        }
    }
}

fn resolve_sync_reference(
    reference: &str,
    image_cache_root: &Path,
    buildah: &impl BuildahCommands,
) -> Result<String> {
    match resolve_local_selector(image_cache_root, buildah, reference)? {
        SelectorResolution::One(row) => row.sync_reference().ok_or_else(|| {
            anyhow!("image selector '{reference}' matched a row without a usable Buildah reference")
        }),
        SelectorResolution::Multiple(rows) => bail!(
            "image selector '{reference}' matched multiple local rows; refusing to sync; candidates: {}",
            format_selector_candidates(&rows)
        ),
        SelectorResolution::None => Ok(reference.to_owned()),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BuildahImageRow {
    repository: String,
    tag: String,
    image_id: Option<String>,
    digest: Option<String>,
}

impl BuildahImageRow {
    fn reference(&self) -> Option<String> {
        named_reference(&self.repository, &self.tag)
    }
}

impl ImageCacheListEntry {
    fn cached(
        digest_key: String,
        digest: Option<String>,
        selected_reference: Option<String>,
        status: ImageCacheEntryStatus,
        buildah_status: BuildahMatchStatus,
        entry_dir: PathBuf,
        image_id: Option<String>,
    ) -> Self {
        let (repository, tag) = selected_reference
            .as_deref()
            .map(split_reference)
            .unwrap_or_else(|| ("<unknown>".to_owned(), "<unknown>".to_owned()));
        Self {
            digest_key: Some(digest_key),
            digest,
            selected_reference,
            repository,
            tag,
            image_id,
            status,
            buildah_status,
            entry_dir: Some(entry_dir),
            short_path: None,
        }
    }

    fn with_buildah_row(&self, buildah_row: &BuildahImageRow) -> Self {
        let mut entry = self.clone();
        entry.repository = buildah_row.repository.clone();
        entry.tag = buildah_row.tag.clone();
        entry.image_id = buildah_row.image_id.clone();
        entry
    }

    fn cache_sort_key(&self) -> String {
        self.digest_key
            .clone()
            .or_else(|| self.digest.clone())
            .or_else(|| self.image_id.clone())
            .unwrap_or_else(|| format!("{}:{}", self.repository, self.tag))
    }

    fn sync_reference(&self) -> Option<String> {
        self.selected_reference
            .clone()
            .or_else(|| named_reference(&self.repository, &self.tag))
            .or_else(|| self.image_id.clone())
    }
}

enum SelectorResolution {
    None,
    One(ImageCacheListEntry),
    Multiple(Vec<ImageCacheListEntry>),
}

fn read_buildah_inventory(buildah: &impl BuildahCommands) -> Vec<BuildahImageRow> {
    const FORMAT: &str = "{{.Name}}|{{.Tag}}|{{.ID}}|{{.Digest}}";
    let output = match buildah.run(&[
        "images",
        "--all",
        "--digests",
        "--noheading",
        "--format",
        FORMAT,
    ]) {
        Ok(output) => output,
        Err(err) => {
            eprintln!("loftd: buildah inventory failed: {err}");
            return Vec::new();
        }
    };
    output
        .lines()
        .filter_map(parse_buildah_inventory_line)
        .collect()
}

fn parse_buildah_inventory_line(line: &str) -> Option<BuildahImageRow> {
    let mut fields = line.split('|');
    let repository = normalize_visible_field(fields.next()?, "<none>");
    let tag = normalize_visible_field(fields.next()?, "<none>");
    let image_id = normalize_optional_field(fields.next()?);
    let digest = normalize_digest_field(fields.next().unwrap_or_default());
    Some(BuildahImageRow {
        repository,
        tag,
        image_id,
        digest,
    })
}
fn normalize_visible_field(value: &str, default: &str) -> String {
    let value = value.trim();
    if value.is_empty() || value == "<no value>" {
        default.to_owned()
    } else {
        value.to_owned()
    }
}

fn normalize_optional_field(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty() && value != "<none>" && value != "<no value>").then(|| value.to_owned())
}

fn normalize_digest_field(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() || value == "<none>" || value == "<no value>" {
        return None;
    }
    Some(value.to_owned())
}

fn enrich_list_entries(
    mut cache_entries: Vec<ImageCacheListEntry>,
    buildah_rows: Vec<BuildahImageRow>,
) -> Vec<ImageCacheListEntry> {
    cache_entries.sort_by_key(|entry| entry.cache_sort_key());
    let mut used_buildah_rows = BTreeSet::new();
    let mut matched_cache_entries = BTreeSet::new();
    let mut entries = Vec::new();

    // Pass 1: digest match (existing behavior)
    for (entry_idx, cache_entry) in cache_entries.iter().enumerate() {
        let matching_indexes = buildah_rows
            .iter()
            .enumerate()
            .filter_map(|(index, buildah_row)| {
                (cache_entry.digest.is_some() && cache_entry.digest == buildah_row.digest)
                    .then_some(index)
            })
            .collect::<Vec<_>>();

        if matching_indexes.is_empty() {
            continue;
        }
        matched_cache_entries.insert(entry_idx);
        for index in matching_indexes {
            used_buildah_rows.insert(index);
            entries.push(cache_entry.with_buildah_row(&buildah_rows[index]));
        }
    }

    // Pass 2: reference match for unmatched entries without image_id.
    // Entries with image_id are handled by post-reconciliation.
    for (entry_idx, cache_entry) in cache_entries.iter().enumerate() {
        if matched_cache_entries.contains(&entry_idx) {
            continue;
        }
        if cache_entry.image_id.is_some() {
            continue;
        }
        let Some(ref reference) = cache_entry.selected_reference else {
            continue;
        };
        let matching_index = buildah_rows.iter().enumerate().find_map(|(index, row)| {
            if used_buildah_rows.contains(&index) {
                return None;
            }
            (row.reference().as_deref() == Some(reference.as_str())).then_some(index)
        });
        let Some(index) = matching_index else {
            continue;
        };
        used_buildah_rows.insert(index);
        matched_cache_entries.insert(entry_idx);
        entries.push(cache_entry.with_buildah_row(&buildah_rows[index]));
    }

    // Pass 3: reconcile cached entries against buildah by image_id.

    // Each cached entry's TAG/REPOSITORY initially comes from stale
    // split_reference(selected_image). Buildah knows the current
    // state — look it up by image_id prefix instead.
    for (entry_idx, cache_entry) in cache_entries.iter().enumerate() {
        if matched_cache_entries.contains(&entry_idx) {
            continue;
        }
        let Some(ref image_id) = cache_entry.image_id else {
            entries.push(cache_entry.clone());
            continue;
        };
        if let Some(index) = buildah_rows
            .iter()
            .enumerate()
            .filter_map(|(index, row)| {
                if used_buildah_rows.contains(&index) {
                    return None;
                }
                row.image_id
                    .as_deref()
                    .is_some_and(|row_id| image_id.starts_with(row_id))
                    .then_some(index)
            })
            .next()
        {
            matched_cache_entries.insert(entry_idx);
            used_buildah_rows.insert(index);
            entries.push(cache_entry.with_buildah_row(&buildah_rows[index]));
        } else {
            entries.push(cache_entry.clone());
        }
    }

    // Pass 4: for any cached entry where image_id doesn't match a buildah
    // row (consumed or not), the image no longer exists — mark <none>.
    // For entries matched in Pass 3, this is a no-op (same row found).
    if !buildah_rows.is_empty() {
        let mut entry_pos: usize = 0;
        while entry_pos < entries.len() {
            let entry = &mut entries[entry_pos];
            if let Some(ref image_id) = entry.image_id {
                if let Some(row) = buildah_rows.iter().find(|row| {
                    row.image_id
                        .as_deref()
                        .is_some_and(|row_id| image_id.starts_with(row_id))
                }) {
                    entry.repository = row.repository.clone();
                    entry.tag = row.tag.clone();
                } else {
                    entry.repository = "<none>".to_owned();
                    entry.tag = "<none>".to_owned();
                }
            }
            entry_pos += 1;
        }
    }
    entries
}

fn resolve_local_selector(
    image_cache_root: &Path,
    buildah: &impl BuildahCommands,
    selector: &str,
) -> Result<SelectorResolution> {
    let rows = list_image_cache(image_cache_root, buildah)?;
    let mut matched_indexes = BTreeSet::new();
    for (index, row) in rows.iter().enumerate() {
        if row_matches_selector(row, selector) {
            matched_indexes.insert(index);
        }
    }

    let matched = matched_indexes
        .into_iter()
        .map(|index| rows[index].clone())
        .collect::<Vec<_>>();
    Ok(match matched.len() {
        0 => SelectorResolution::None,
        1 => SelectorResolution::One(matched.into_iter().next().expect("one row")),
        _ => SelectorResolution::Multiple(matched),
    })
}

fn row_matches_selector(row: &ImageCacheListEntry, selector: &str) -> bool {
    row.digest.as_deref().is_some_and(|digest| {
        digest_token_matches(digest, selector)
            || digest_hex(digest).is_some_and(|hex| prefix_or_exact(selector, hex, 3))
    }) || row
        .digest_key
        .as_deref()
        .is_some_and(|key| digest_key_token_matches(key, selector))
        || row
            .selected_reference
            .as_deref()
            .is_some_and(|reference| prefix_or_exact(selector, reference, 2))
        || prefix_or_exact_visible(selector, &row.repository, 2)
        || prefix_or_exact_visible(selector, &row.tag, 2)
        || named_reference(&row.repository, &row.tag)
            .as_deref()
            .is_some_and(|reference| prefix_or_exact(selector, reference, 2))
        || row.image_id.as_deref().is_some_and(|image_id| {
            prefix_or_exact(selector, image_id, 3)
                || prefix_or_exact(selector, &short_token(image_id), 3)
        })
}

fn digest_token_matches(digest: &str, selector: &str) -> bool {
    selector == digest
        || selector
            .strip_prefix("sha256:")
            .is_some_and(|hex_selector| {
                digest_hex(digest).is_some_and(|hex| prefix_or_exact(hex_selector, hex, 3))
            })
}

fn digest_key_token_matches(key: &str, selector: &str) -> bool {
    selector == key
        || selector
            .strip_prefix("sha256-")
            .is_some_and(|hex_selector| {
                key.strip_prefix("sha256-")
                    .is_some_and(|hex| prefix_or_exact(hex_selector, hex, 3))
            })
}

fn prefix_or_exact_visible(selector: &str, token: &str, min_prefix: usize) -> bool {
    if matches!(token, "<none>" | "<unknown>") {
        selector == token
    } else {
        prefix_or_exact(selector, token, min_prefix)
    }
}

fn prefix_or_exact(selector: &str, token: &str, min_prefix: usize) -> bool {
    selector == token || (selector.len() >= min_prefix && token.starts_with(selector))
}

fn exact_digest_entry(
    image_cache_root: &Path,
    target: &str,
) -> Result<Option<BtrfsImageCacheEntry>> {
    if target.starts_with("sha256:") {
        let entry = BtrfsImageCacheEntry::new(image_cache_root, target)?;
        return Ok(entry.entry_dir.exists().then_some(entry));
    }
    if target.starts_with("sha256-")
        && let Some(digest) = digest_from_digest_key_path_component(target)
    {
        validate_digest_key_target(target)?;
        let entry = BtrfsImageCacheEntry::new(image_cache_root, &digest)?;
        return Ok(entry.entry_dir.exists().then_some(entry));
    }
    Ok(None)
}

fn split_reference(reference: &str) -> (String, String) {
    if matches!(reference, "<none>" | "<unknown>") {
        return (reference.to_owned(), reference.to_owned());
    }
    if let Some((name, _digest)) = reference.split_once('@') {
        return (name.to_owned(), "<none>".to_owned());
    }
    let last_slash = reference.rfind('/');
    let last_colon = reference.rfind(':');
    if let Some(colon) = last_colon
        && last_slash.is_none_or(|slash| colon > slash)
    {
        return (
            reference[..colon].to_owned(),
            reference[colon + 1..].to_owned(),
        );
    }
    (reference.to_owned(), "<none>".to_owned())
}

fn named_reference(repository: &str, tag: &str) -> Option<String> {
    if matches!(repository, "" | "<none>" | "<unknown>")
        || matches!(tag, "" | "<none>" | "<unknown>")
    {
        None
    } else {
        Some(format!("{repository}:{tag}"))
    }
}

fn digest_hex(digest: &str) -> Option<&str> {
    let (algorithm, hex) = digest.split_once(':')?;
    (algorithm == "sha256" && !hex.is_empty()).then_some(hex)
}

fn short_digest(digest: Option<&str>) -> String {
    digest
        .and_then(digest_hex)
        .map(short_token)
        .unwrap_or_else(|| "<unknown>".to_owned())
}

fn short_image_id(image_id: Option<&str>) -> String {
    image_id
        .map(short_token)
        .unwrap_or_else(|| "<none>".to_owned())
}

fn short_token(token: &str) -> String {
    token.chars().take(12).collect()
}

fn format_selector_candidates(rows: &[ImageCacheListEntry]) -> String {
    rows.iter()
        .map(|row| {
            let reference = named_reference(&row.repository, &row.tag)
                .or_else(|| row.selected_reference.clone())
                .unwrap_or_else(|| format!("{}:{}", row.repository, row.tag));
            format!(
                "{} image-id {} digest {} cache {}",
                reference,
                short_image_id(row.image_id.as_deref()),
                short_digest(row.digest.as_deref()),
                row.status.as_str()
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn read_cached_metadata_for_remove(
    entry: &BtrfsImageCacheEntry,
) -> Result<super::ImageSourceRootfs> {
    let metadata = fs::read_to_string(&entry.metadata_path).with_context(|| {
        format!(
            "failed to read loftd image cache metadata '{}'",
            entry.metadata_path.display()
        )
    })?;
    parse_cache_metadata(&metadata, &entry.rootfs_path)
}

fn buildah_match_status(
    reference: Option<&str>,
    expected_digest: Option<&str>,
    buildah: &impl BuildahCommands,
) -> BuildahMatchStatus {
    let Some(reference) = reference else {
        return BuildahMatchStatus::NoReference;
    };
    let Some(expected_digest) = expected_digest else {
        return BuildahMatchStatus::NoCacheDigest;
    };
    match inspect_image_digest(reference, buildah) {
        Ok(Some(actual_digest)) if actual_digest == expected_digest => BuildahMatchStatus::Match,
        Ok(Some(_)) => BuildahMatchStatus::DigestMismatch,
        Ok(None) | Err(_) => BuildahMatchStatus::MissingOrDigestless,
    }
}

fn remove_guarded_local_image(
    reference: &str,
    expected_digest: &str,
    buildah: &impl BuildahCommands,
) -> Result<LocalImageRemoval> {
    let Some(actual_digest) = inspect_image_digest(reference, buildah)? else {
        return Ok(LocalImageRemoval::Skipped {
            reason: format!(
                "Buildah image '{reference}' is missing, digestless, or ambiguous; expected digest {expected_digest}"
            ),
        });
    };
    if actual_digest != expected_digest {
        return Ok(LocalImageRemoval::Skipped {
            reason: format!(
                "Buildah image '{reference}' digest {actual_digest} does not match cache digest {expected_digest}"
            ),
        });
    }
    buildah
        .run(&["rmi", reference])
        .with_context(|| format!("failed to remove matching Buildah image '{reference}'"))?;
    Ok(LocalImageRemoval::Removed {
        reference: reference.to_owned(),
    })
}

fn render_list_stdout(entries: &[ImageCacheListEntry]) -> String {
    let headers = [
        "REPOSITORY",
        "TAG",
        "IMAGE ID",
        "DIGEST",
        "CACHE",
        "BUILDAH",
        "PATH",
    ];
    let mut widths: [usize; 7] = [0; 7];
    for (i, hdr) in headers.iter().enumerate() {
        widths[i] = hdr.len();
    }
    for entry in entries {
        let img_id = short_image_id(entry.image_id.as_deref());
        let digest = short_digest(entry.digest.as_deref());
        let status = entry.status.as_str();
        let buildah = entry.buildah_status.as_str();
        let path = entry.short_path.as_deref().unwrap_or("<none>");
        widths[0] = widths[0].max(entry.repository.len());
        widths[1] = widths[1].max(entry.tag.len());
        widths[2] = widths[2].max(img_id.len());
        widths[3] = widths[3].max(digest.len());
        widths[4] = widths[4].max(status.len());
        widths[5] = widths[5].max(buildah.len());
        widths[6] = widths[6].max(path.len());
    }
    let mut output = String::new();
    output.push_str(&format!(
        "{:<w0$}  {:<w1$}  {:<w2$}  {:<w3$}  {:<w4$}  {:<w5$}  {:<w6$}\n",
        headers[0],
        headers[1],
        headers[2],
        headers[3],
        headers[4],
        headers[5],
        headers[6],
        w0 = widths[0],
        w1 = widths[1],
        w2 = widths[2],
        w3 = widths[3],
        w4 = widths[4],
        w5 = widths[5],
        w6 = widths[6],
    ));
    for entry in entries {
        let repo = &entry.repository;
        let tag = &entry.tag;
        let img_id = short_image_id(entry.image_id.as_deref());
        let digest = short_digest(entry.digest.as_deref());
        let status = entry.status.as_str();
        let buildah = entry.buildah_status.as_str();
        let path = entry.short_path.as_deref().unwrap_or("<none>");
        output.push_str(&format!(
            "{:<w0$}  {:<w1$}  {:<w2$}  {:<w3$}  {:<w4$}  {:<w5$}  {:<w6$}\n",
            repo,
            tag,
            img_id,
            digest,
            status,
            buildah,
            path,
            w0 = widths[0],
            w1 = widths[1],
            w2 = widths[2],
            w3 = widths[3],
            w4 = widths[4],
            w5 = widths[5],
            w6 = widths[6],
        ));
    }
    output
}

fn validate_digest_key_target(target: &str) -> Result<()> {
    if target == "."
        || target == ".."
        || target.contains('/')
        || target.contains('\\')
        || !target
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        bail!("loftd image cache digest key '{target}' is not safe");
    }
    Ok(())
}

fn digest_from_digest_key(entry_dir: &Path) -> Option<String> {
    entry_dir
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(digest_from_digest_key_path_component)
}

fn digest_from_digest_key_path_component(key: &str) -> Option<String> {
    let (algorithm, value) = key.split_once('-')?;
    if algorithm.is_empty() || value.is_empty() {
        return None;
    }
    Some(format!("{algorithm}:{value}"))
}

struct ImageCacheStagingDir {
    path: PathBuf,
}

impl ImageCacheStagingDir {
    fn create(image_cache_root: &Path) -> Result<Self> {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        let path = image_cache_root
            .join(STAGING_DIR)
            .join(format!("sync-{}-{nonce}", process::id()));
        fs::create_dir_all(&path).with_context(|| {
            format!(
                "failed to create loftd image cache staging directory '{}'",
                path.display()
            )
        })?;
        Ok(Self { path })
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn cleanup(self, btrfs: &impl BtrfsRootfsCommands) -> Result<()> {
        let rootfs_path = self.path.join(CACHE_ROOTFS_DIR);
        if rootfs_path.exists() {
            btrfs
                .delete_btrfs_subvolume(&rootfs_path)
                .with_context(|| {
                    format!(
                        "failed to delete loftd image cache staging rootfs '{}'",
                        rootfs_path.display()
                    )
                })?;
        }
        if self.path.exists() {
            remove_cache_entry_tree(&self.path).with_context(|| {
                format!(
                    "failed to remove loftd image cache staging directory '{}'",
                    self.path.display()
                )
            })?;
        }
        Ok(())
    }
}
