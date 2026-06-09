use anyhow::{Context, Result, anyhow, bail};
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
    pub(crate) digest_key: String,
    pub(crate) digest: Option<String>,
    pub(crate) selected_reference: Option<String>,
    pub(crate) status: ImageCacheEntryStatus,
    pub(crate) buildah_status: BuildahMatchStatus,
    pub(crate) entry_dir: PathBuf,
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
    if reference.trim().is_empty() {
        bail!("image reference must not be empty");
    }

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
    if !cache_dir.exists() {
        return Ok(Vec::new());
    }

    let mut entries = Vec::new();
    for dir_entry in fs::read_dir(&cache_dir)
        .with_context(|| format!("failed to read loftd image cache '{}'", cache_dir.display()))?
    {
        let dir_entry = dir_entry?;
        let file_type = dir_entry.file_type()?;
        if !file_type.is_dir() {
            continue;
        }
        let digest_key = dir_entry.file_name().to_string_lossy().to_string();
        entries.push(read_list_entry(digest_key, dir_entry.path(), buildah));
    }
    entries.sort_by(|left, right| left.digest_key.cmp(&right.digest_key));
    Ok(entries)
}

fn read_list_entry(
    digest_key: String,
    entry_dir: PathBuf,
    buildah: &impl BuildahCommands,
) -> ImageCacheListEntry {
    let rootfs_path = entry_dir.join(CACHE_ROOTFS_DIR);
    let metadata_path = entry_dir.join(CACHE_METADATA_FILE);
    if !rootfs_path.is_dir() || !metadata_path.is_file() {
        return ImageCacheListEntry {
            digest_key,
            digest: digest_from_digest_key(&entry_dir),
            selected_reference: None,
            status: ImageCacheEntryStatus::Incomplete,
            buildah_status: BuildahMatchStatus::NoReference,
            entry_dir,
        };
    }

    match fs::read_to_string(&metadata_path)
        .with_context(|| format!("failed to read '{}'", metadata_path.display()))
        .and_then(|metadata| parse_cache_metadata(&metadata, &rootfs_path))
    {
        Ok(rootfs) => {
            let digest = rootfs.image_digest;
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
            ImageCacheListEntry {
                digest_key,
                digest,
                selected_reference: Some(selected_reference),
                status,
                buildah_status,
                entry_dir,
            }
        }
        Err(_) => ImageCacheListEntry {
            digest_key,
            digest: digest_from_digest_key(&entry_dir),
            selected_reference: None,
            status: ImageCacheEntryStatus::Invalid,
            buildah_status: BuildahMatchStatus::InvalidCache,
            entry_dir,
        },
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

    let entry = resolve_remove_target(image_cache_root, target)?;
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

fn resolve_remove_target(image_cache_root: &Path, target: &str) -> Result<BtrfsImageCacheEntry> {
    if target.contains(':') {
        return BtrfsImageCacheEntry::new(image_cache_root, target);
    }
    validate_digest_key_target(target)?;
    let digest = digest_from_digest_key_path_component(target).ok_or_else(|| {
        anyhow!("loftd image cache target '{target}' is not a supported digest key")
    })?;
    BtrfsImageCacheEntry::new(image_cache_root, &digest)
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
    let mut output = String::from("DIGEST_KEY\tDIGEST\tIMAGE\tSTATUS\tBUILDAH\tPATH\n");
    for entry in entries {
        output.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\t{}\n",
            entry.digest_key,
            entry.digest.as_deref().unwrap_or("<unknown>"),
            entry.selected_reference.as_deref().unwrap_or("<unknown>"),
            entry.status.as_str(),
            entry.buildah_status.as_str(),
            entry.entry_dir.display()
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
