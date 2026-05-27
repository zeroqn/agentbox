use anyhow::{Context, Result, anyhow};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::DEFAULT_IMAGE;

const BLOBS_DIR: &str = "blobs";
const REFS_DIR: &str = "refs";
const ROOTFS_DIR: &str = "rootfs";
const COMPATIBILITY_MARKER: &str = "agentbox-compatible";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ImageReference(String);

impl ImageReference {
    pub(crate) fn from_cli(image: Option<&str>) -> Self {
        Self(image.unwrap_or(DEFAULT_IMAGE).to_owned())
    }

    fn as_str(&self) -> &str {
        &self.0
    }

    fn digest(&self) -> Option<ImageDigest> {
        self.0
            .split_once('@')
            .and_then(|(_, digest)| ImageDigest::parse(digest).ok())
    }

    fn ref_key(&self) -> String {
        percent_encode(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ImageDigest(String);

impl ImageDigest {
    pub(crate) fn parse(input: &str) -> Result<Self> {
        let (algorithm, value) = input
            .split_once(':')
            .ok_or_else(|| anyhow!("image digest '{input}' must use algorithm:value form"))?;
        if algorithm.is_empty() || value.is_empty() {
            anyhow::bail!("image digest '{input}' must include non-empty algorithm and value");
        }
        if algorithm.contains('/')
            || value.contains('/')
            || algorithm.contains("..")
            || value.contains("..")
        {
            anyhow::bail!("image digest '{input}' is not safe for cache paths");
        }
        Ok(Self(format!("{algorithm}:{value}")))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }

    fn path_components(&self) -> (&str, &str) {
        self.0
            .split_once(':')
            .expect("ImageDigest is validated at construction")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ImageCacheEntry {
    pub(crate) reference: ImageReference,
    pub(crate) digest: ImageDigest,
    pub(crate) rootfs: PathBuf,
    pub(crate) compatibility: ImageCompatibility,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ImageCompatibility {
    Agentbox,
    Unsupported,
}

impl ImageCacheEntry {
    pub(crate) fn ensure_agentbox_compatible(&self) -> Result<()> {
        match self.compatibility {
            ImageCompatibility::Agentbox => Ok(()),
            ImageCompatibility::Unsupported => Err(anyhow!(
                "image '{}' is not an agentbox-compatible microvm image; cache entry '{}' is missing compatibility metadata",
                self.reference.as_str(),
                self.digest.as_str()
            )),
        }
    }
}

pub(crate) trait BuildahRunner {
    fn ingest(&self, reference: &ImageReference, cache_root: &Path) -> Result<ImageDigest>;
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct HostBuildahRunner;

impl BuildahRunner for HostBuildahRunner {
    fn ingest(&self, reference: &ImageReference, cache_root: &Path) -> Result<ImageDigest> {
        let output = Command::new("buildah")
            .arg("--version")
            .output()
            .map_err(|err| match err.kind() {
                std::io::ErrorKind::NotFound => anyhow!(
                    "buildah is required to ingest '{}' into the experimental microvm image cache; install buildah or pre-populate '{}'",
                    reference.as_str(),
                    cache_root.display()
                ),
                _ => err.into(),
            })
            .with_context(|| format!("failed to run buildah for '{}'", reference.as_str()))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            if stderr.is_empty() {
                anyhow::bail!("buildah is not usable for '{}'", reference.as_str());
            }
            anyhow::bail!(
                "buildah is not usable for '{}': {stderr}",
                reference.as_str()
            );
        }

        anyhow::bail!(
            "buildah is available, but OCI unpack into the experimental microvm image cache is not implemented yet for '{}'",
            reference.as_str()
        )
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ImageCache {
    root: PathBuf,
}

impl ImageCache {
    pub(crate) fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub(crate) fn ensure(
        &self,
        reference: ImageReference,
        runner: &impl BuildahRunner,
    ) -> Result<ImageCacheEntry> {
        if let Some(digest) = reference.digest()
            && let Some(entry) = self.entry_if_cached(reference.clone(), digest)?
        {
            return Ok(entry);
        }

        if let Some(digest) = self.lookup_ref_digest(&reference)?
            && let Some(entry) = self.entry_if_cached(reference.clone(), digest)?
        {
            return Ok(entry);
        }

        let digest = runner.ingest(&reference, &self.root)?;
        self.record_ref_digest(&reference, &digest)?;
        self.entry_if_cached(reference, digest)?.ok_or_else(|| {
            anyhow!("buildah ingested image but no microvm cache rootfs was found afterward")
        })
    }

    pub(crate) fn entry_path(&self, digest: &ImageDigest) -> PathBuf {
        let (algorithm, value) = digest.path_components();
        self.root.join(BLOBS_DIR).join(algorithm).join(value)
    }

    #[cfg(test)]
    pub(crate) fn record_ref_digest(
        &self,
        reference: &ImageReference,
        digest: &ImageDigest,
    ) -> Result<()> {
        self.write_ref_digest(reference, digest)
    }

    #[cfg(not(test))]
    fn record_ref_digest(&self, reference: &ImageReference, digest: &ImageDigest) -> Result<()> {
        self.write_ref_digest(reference, digest)
    }

    fn write_ref_digest(&self, reference: &ImageReference, digest: &ImageDigest) -> Result<()> {
        let refs_dir = self.root.join(REFS_DIR);
        fs::create_dir_all(&refs_dir).with_context(|| {
            format!(
                "failed to create microvm image ref index '{}'",
                refs_dir.display()
            )
        })?;
        fs::write(refs_dir.join(reference.ref_key()), digest.as_str()).with_context(|| {
            format!(
                "failed to record microvm image ref '{}'",
                reference.as_str()
            )
        })
    }

    fn lookup_ref_digest(&self, reference: &ImageReference) -> Result<Option<ImageDigest>> {
        let path = self.root.join(REFS_DIR).join(reference.ref_key());
        if !path.exists() {
            return Ok(None);
        }
        let digest = fs::read_to_string(&path).with_context(|| {
            format!(
                "failed to read microvm image ref '{}': {}",
                reference.as_str(),
                path.display()
            )
        })?;
        Ok(Some(ImageDigest::parse(digest.trim())?))
    }

    fn entry_if_cached(
        &self,
        reference: ImageReference,
        digest: ImageDigest,
    ) -> Result<Option<ImageCacheEntry>> {
        let entry_dir = self.entry_path(&digest);
        let rootfs = entry_dir.join(ROOTFS_DIR);
        if !rootfs.is_dir() {
            return Ok(None);
        }
        let compatibility = if entry_dir.join(COMPATIBILITY_MARKER).exists() {
            ImageCompatibility::Agentbox
        } else {
            ImageCompatibility::Unsupported
        };
        Ok(Some(ImageCacheEntry {
            reference,
            digest,
            rootfs,
            compatibility,
        }))
    }
}

fn percent_encode(input: &str) -> String {
    let mut encoded = String::new();
    for byte in input.bytes() {
        match byte {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'.' | b'_' | b'-' => {
                encoded.push(byte as char);
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    struct CountingRunner {
        calls: Cell<u32>,
        result: Result<ImageDigest, &'static str>,
    }

    impl CountingRunner {
        fn unavailable() -> Self {
            Self {
                calls: Cell::new(0),
                result: Err("buildah unavailable"),
            }
        }

        fn calls(&self) -> u32 {
            self.calls.get()
        }
    }

    impl BuildahRunner for CountingRunner {
        fn ingest(&self, _reference: &ImageReference, _cache_root: &Path) -> Result<ImageDigest> {
            self.calls.set(self.calls.get() + 1);
            match &self.result {
                Ok(digest) => Ok(digest.clone()),
                Err(message) => Err(anyhow!(*message)),
            }
        }
    }

    fn write_cached_root(cache: &ImageCache, digest: &ImageDigest) {
        let entry_dir = cache.entry_path(digest);
        fs::create_dir_all(entry_dir.join(ROOTFS_DIR)).expect("rootfs dir should be created");
        fs::write(entry_dir.join(COMPATIBILITY_MARKER), "agentbox\n")
            .expect("marker should be written");
    }

    #[test]
    fn digest_cache_entry_path_is_digest_keyed_not_reference_keyed() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let cache = ImageCache::new(temp.path().join("images"));
        let digest = ImageDigest::parse("sha256:abc123").expect("digest should parse");

        assert_eq!(
            cache.entry_path(&digest),
            temp.path()
                .join("images")
                .join("blobs")
                .join("sha256")
                .join("abc123")
        );
    }

    #[test]
    fn digest_pinned_cache_hit_does_not_call_buildah() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let cache = ImageCache::new(temp.path().join("images"));
        let digest = ImageDigest::parse("sha256:abc123").expect("digest should parse");
        write_cached_root(&cache, &digest);
        let runner = CountingRunner::unavailable();

        let entry = cache
            .ensure(
                ImageReference::from_cli(Some("ghcr.io/example/agentbox@sha256:abc123")),
                &runner,
            )
            .expect("digest-pinned cache hit should not need buildah");

        assert_eq!(runner.calls(), 0);
        assert_eq!(entry.digest, digest);
        assert_eq!(entry.compatibility, ImageCompatibility::Agentbox);
    }

    #[test]
    fn mutable_tag_cache_hit_uses_local_ref_metadata_without_buildah() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let cache = ImageCache::new(temp.path().join("images"));
        let reference = ImageReference::from_cli(Some("ghcr.io/example/agentbox:dev"));
        let digest = ImageDigest::parse("sha256:def456").expect("digest should parse");
        write_cached_root(&cache, &digest);
        cache
            .record_ref_digest(&reference, &digest)
            .expect("ref metadata should be recorded");
        let runner = CountingRunner::unavailable();

        let entry = cache
            .ensure(reference, &runner)
            .expect("tag metadata cache hit should not need buildah");

        assert_eq!(runner.calls(), 0);
        assert_eq!(entry.digest, digest);
    }

    #[test]
    fn cache_miss_reports_buildah_requirement() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let cache = ImageCache::new(temp.path().join("images"));
        let runner = CountingRunner::unavailable();

        let error = cache
            .ensure(
                ImageReference::from_cli(Some("ghcr.io/example/agentbox:missing")),
                &runner,
            )
            .expect_err("cache miss should require buildah ingestion");

        assert_eq!(runner.calls(), 1);
        assert!(format!("{error:#}").contains("buildah unavailable"));
    }

    #[test]
    fn unsupported_cache_entry_fails_as_image_compatibility_error() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let cache = ImageCache::new(temp.path().join("images"));
        let digest = ImageDigest::parse("sha256:abc123").expect("digest should parse");
        fs::create_dir_all(cache.entry_path(&digest).join(ROOTFS_DIR))
            .expect("rootfs dir should be created without compatibility marker");
        let runner = CountingRunner::unavailable();

        let entry = cache
            .ensure(
                ImageReference::from_cli(Some("ghcr.io/example/plain@sha256:abc123")),
                &runner,
            )
            .expect("cache hit should be returned before compatibility check");
        let error = entry
            .ensure_agentbox_compatible()
            .expect_err("missing compatibility marker should fail before boot");

        assert!(format!("{error:#}").contains("not an agentbox-compatible microvm image"));
        assert_eq!(runner.calls(), 0);
    }
}
