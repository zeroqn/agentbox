use anyhow::{Context, Result, anyhow};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::DEFAULT_IMAGE;
use crate::runtime::microvm::guest_init::find_agentbox_guest_init;
use crate::runtime::microvm::storage::copy_rootfs_tree;

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

pub(crate) trait BuildahCommandRunner {
    fn run(&self, args: &[&str]) -> Result<String>;
}

#[derive(Debug, Clone, Copy)]
struct HostBuildahCommandRunner;

impl BuildahCommandRunner for HostBuildahCommandRunner {
    fn run(&self, args: &[&str]) -> Result<String> {
        let output = Command::new("buildah")
            .args(args)
            .stdin(Stdio::null())
            .output()
            .map_err(|err| match err.kind() {
                std::io::ErrorKind::NotFound => anyhow!(
                    "buildah is required to ingest images into the experimental microvm image cache"
                ),
                _ => err.into(),
            })
            .with_context(|| format!("failed to run buildah {}", args.join(" ")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            if stderr.is_empty() {
                anyhow::bail!("buildah {} failed", args.join(" "));
            }
            anyhow::bail!("buildah {} failed: {stderr}", args.join(" "));
        }
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }
}

struct BuildahCommandIngestor<'a, C> {
    commands: &'a C,
}

impl<'a, C> BuildahCommandIngestor<'a, C> {
    fn new(commands: &'a C) -> Self {
        Self { commands }
    }
}

impl<C: BuildahCommandRunner> BuildahRunner for BuildahCommandIngestor<'_, C> {
    fn ingest(&self, reference: &ImageReference, cache_root: &Path) -> Result<ImageDigest> {
        ingest_with_buildah_commands(reference, cache_root, self.commands)
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct HostBuildahRunner;

impl BuildahRunner for HostBuildahRunner {
    fn ingest(&self, reference: &ImageReference, cache_root: &Path) -> Result<ImageDigest> {
        BuildahCommandIngestor::new(&HostBuildahCommandRunner).ingest(reference, cache_root)
    }
}

fn ingest_with_buildah_commands(
    reference: &ImageReference,
    cache_root: &Path,
    commands: &impl BuildahCommandRunner,
) -> Result<ImageDigest> {
    commands.run(&["--version"]).with_context(|| {
        format!(
            "failed to verify buildah for microvm image '{}'; install buildah or pre-populate '{}'",
            reference.as_str(),
            cache_root.display()
        )
    })?;

    let container_id = trim_required(
        commands
            .run(&["from", reference.as_str()])
            .with_context(|| {
                format!(
                    "failed to create buildah working container for '{}'",
                    reference.as_str()
                )
            })?,
        "buildah from did not return a working container id",
    )?;

    let mut mounted = false;
    let ingest_result = (|| {
        let digest_output = commands
            .run(&[
                "inspect",
                "--format",
                "{{.FromImageDigest}}",
                container_id.as_str(),
            ])
            .with_context(|| {
                format!(
                    "failed to inspect buildah digest for microvm image '{}'",
                    reference.as_str()
                )
            })?;
        let digest = ImageDigest::parse(&trim_required(
            digest_output,
            "buildah inspect did not return an image digest",
        )?)?;
        if let Some(expected) = reference.digest()
            && expected != digest
        {
            anyhow::bail!(
                "buildah resolved '{}' to digest '{}', but the image reference requested '{}'",
                reference.as_str(),
                digest.as_str(),
                expected.as_str()
            );
        }

        let mount_path = PathBuf::from(trim_required(
            commands
                .run(&["mount", container_id.as_str()])
                .with_context(|| {
                    format!(
                        "failed to mount buildah working container for '{}'",
                        reference.as_str()
                    )
                })?,
            "buildah mount did not return a mount path",
        )?);
        mounted = true;
        finalize_cache_entry(cache_root, &digest, &mount_path)?;
        Ok(digest)
    })();

    let cleanup_result = cleanup_buildah_container(commands, &container_id, mounted);
    match (ingest_result, cleanup_result) {
        (Ok(digest), Ok(())) => Ok(digest),
        (Ok(_), Err(cleanup_err)) => Err(cleanup_err),
        (Err(ingest_err), Ok(())) => Err(ingest_err),
        (Err(ingest_err), Err(cleanup_err)) => Err(cleanup_err.context(format!(
            "buildah cleanup failed after image ingestion error: {ingest_err:#}"
        ))),
    }
}

fn trim_required(output: String, empty_message: &'static str) -> Result<String> {
    let trimmed = output.trim();
    if trimmed.is_empty() {
        anyhow::bail!(empty_message);
    }
    Ok(trimmed.to_owned())
}

fn finalize_cache_entry(cache_root: &Path, digest: &ImageDigest, mount_path: &Path) -> Result<()> {
    let entry_dir = cache_entry_path(cache_root, digest);
    if entry_is_agentbox_compatible(&entry_dir) {
        return Ok(());
    }
    let parent = entry_dir
        .parent()
        .ok_or_else(|| anyhow!("microvm image cache entry has no parent"))?;
    fs::create_dir_all(parent).with_context(|| {
        format!(
            "failed to create microvm image cache directory '{}'",
            parent.display()
        )
    })?;

    let staging_dir = staging_entry_path(&entry_dir);
    if staging_dir.exists() {
        fs::remove_dir_all(&staging_dir).with_context(|| {
            format!(
                "failed to reset stale microvm image cache staging directory '{}'",
                staging_dir.display()
            )
        })?;
    }
    fs::create_dir_all(&staging_dir).with_context(|| {
        format!(
            "failed to create microvm image cache staging directory '{}'",
            staging_dir.display()
        )
    })?;

    let finalize_result = (|| {
        let staged_rootfs = staging_dir.join(ROOTFS_DIR);
        copy_rootfs_tree(mount_path, &staged_rootfs).with_context(|| {
            format!(
                "failed to copy buildah-mounted rootfs '{}' into microvm image cache '{}'",
                mount_path.display(),
                staged_rootfs.display()
            )
        })?;
        find_agentbox_guest_init(&staged_rootfs).with_context(|| {
            format!(
                "buildah-ingested image digest '{}' is not agentbox-compatible",
                digest.as_str()
            )
        })?;
        fs::write(staging_dir.join(COMPATIBILITY_MARKER), "agentbox\n").with_context(|| {
            format!(
                "failed to write microvm image cache compatibility marker '{}'",
                staging_dir.join(COMPATIBILITY_MARKER).display()
            )
        })?;

        match fs::rename(&staging_dir, &entry_dir) {
            Ok(()) => Ok(()),
            Err(_err) if entry_is_agentbox_compatible(&entry_dir) => {
                fs::remove_dir_all(&staging_dir).ok();
                Ok(())
            }
            Err(err) => Err(err).with_context(|| {
                format!(
                    "failed to finalize microvm image cache entry '{}'",
                    entry_dir.display()
                )
            }),
        }
    })();

    match finalize_result {
        Ok(()) => Ok(()),
        Err(err) => {
            if staging_dir.exists() {
                fs::remove_dir_all(&staging_dir).with_context(|| {
                    format!(
                        "failed to clean microvm image cache staging directory '{}' after error: {err:#}",
                        staging_dir.display()
                    )
                })?;
            }
            Err(err)
        }
    }
}

fn cleanup_buildah_container(
    commands: &impl BuildahCommandRunner,
    container_id: &str,
    mounted: bool,
) -> Result<()> {
    let mut cleanup_errors = Vec::new();
    if mounted && let Err(err) = commands.run(&["umount", container_id]) {
        cleanup_errors.push(format!("{err:#}"));
    }
    if let Err(err) = commands.run(&["rm", container_id]) {
        cleanup_errors.push(format!("{err:#}"));
    }
    if cleanup_errors.is_empty() {
        Ok(())
    } else {
        anyhow::bail!("{}", cleanup_errors.join("; "))
    }
}

fn cache_entry_path(cache_root: &Path, digest: &ImageDigest) -> PathBuf {
    let (algorithm, value) = digest.path_components();
    cache_root.join(BLOBS_DIR).join(algorithm).join(value)
}

fn staging_entry_path(entry_dir: &Path) -> PathBuf {
    let name = entry_dir
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("entry");
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    entry_dir.with_file_name(format!(".{name}.partial-{}-{nanos}", std::process::id()))
}

fn entry_is_agentbox_compatible(entry_dir: &Path) -> bool {
    let rootfs = entry_dir.join(ROOTFS_DIR);
    rootfs.is_dir()
        && entry_dir.join(COMPATIBILITY_MARKER).exists()
        && find_agentbox_guest_init(&rootfs).is_ok()
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
        let entry = self
            .entry_if_cached(reference.clone(), digest.clone())?
            .ok_or_else(|| {
                anyhow!("buildah ingested image but no microvm cache rootfs was found afterward")
            })?;
        self.record_ref_digest(&reference, &digest)?;
        Ok(entry)
    }

    pub(crate) fn entry_path(&self, digest: &ImageDigest) -> PathBuf {
        cache_entry_path(&self.root, digest)
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
    use std::cell::RefCell;
    use std::os::unix::fs::PermissionsExt;

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

    struct FakeBuildahCommands {
        calls: RefCell<Vec<Vec<String>>>,
        mount_root: PathBuf,
        reference: String,
        digest_output: String,
    }

    impl FakeBuildahCommands {
        fn new(mount_root: PathBuf) -> Self {
            Self {
                calls: RefCell::new(Vec::new()),
                mount_root,
                reference: "ghcr.io/example/agentbox:dev".to_owned(),
                digest_output: "sha256:feedface\n".to_owned(),
            }
        }

        fn calls(&self) -> Vec<Vec<String>> {
            self.calls.borrow().clone()
        }

        fn with_reference(mut self, reference: &str) -> Self {
            self.reference = reference.to_owned();
            self
        }

        fn with_digest_output(mut self, digest_output: &str) -> Self {
            self.digest_output = digest_output.to_owned();
            self
        }
    }

    impl BuildahCommandRunner for FakeBuildahCommands {
        fn run(&self, args: &[&str]) -> Result<String> {
            self.calls
                .borrow_mut()
                .push(args.iter().map(|arg| (*arg).to_owned()).collect());
            let from_args = ["from", self.reference.as_str()];
            match args {
                ["--version"] => Ok("buildah version 1.42.0\n".to_owned()),
                other if other == from_args.as_slice() => {
                    Ok("agentbox-working-container\n".to_owned())
                }
                [
                    "inspect",
                    "--format",
                    "{{.FromImageDigest}}",
                    "agentbox-working-container",
                ] => Ok(self.digest_output.clone()),
                ["mount", "agentbox-working-container"] => {
                    Ok(format!("{}\n", self.mount_root.display()))
                }
                ["umount", "agentbox-working-container"] | ["rm", "agentbox-working-container"] => {
                    Ok(String::new())
                }
                other => anyhow::bail!("unexpected buildah args: {other:?}"),
            }
        }
    }

    fn write_guest_init(root: &Path) -> PathBuf {
        let guest_init = root.join("nix/store/hash-agentbox/bin/agentbox-guest-init");
        fs::create_dir_all(guest_init.parent().expect("guest init parent"))
            .expect("guest init parent should be created");
        fs::write(&guest_init, "#!/bin/sh\n").expect("guest init should be written");
        fs::set_permissions(&guest_init, fs::Permissions::from_mode(0o755))
            .expect("guest init should be executable");
        guest_init
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
    fn cache_miss_does_not_record_ref_when_runner_returns_without_rootfs() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let cache = ImageCache::new(temp.path().join("images"));
        let reference = ImageReference::from_cli(Some("ghcr.io/example/agentbox:dev"));
        let digest = ImageDigest::parse("sha256:missingroot").expect("digest should parse");
        let runner = CountingRunner {
            calls: Cell::new(0),
            result: Ok(digest),
        };

        let error = cache
            .ensure(reference.clone(), &runner)
            .expect_err("missing rootfs should fail");

        assert!(format!("{error:#}").contains("no microvm cache rootfs"));
        assert!(
            cache
                .lookup_ref_digest(&reference)
                .expect("ref lookup should work")
                .is_none()
        );
    }

    #[test]
    fn buildah_cache_miss_ingests_rootfs_records_tag_and_marks_compatible() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let mount_root = temp.path().join("buildah-mount");
        write_guest_init(&mount_root);
        fs::create_dir_all(mount_root.join("etc")).expect("etc should be created");
        fs::write(mount_root.join("etc/os-release"), "NAME=agentbox\n")
            .expect("rootfs content should be written");
        let commands = FakeBuildahCommands::new(mount_root);
        let reference = ImageReference::from_cli(Some("ghcr.io/example/agentbox:dev"));
        let cache = ImageCache::new(temp.path().join("images"));
        let runner = BuildahCommandIngestor::new(&commands);

        let entry = cache
            .ensure(reference.clone(), &runner)
            .expect("cache miss should ingest with buildah");
        let digest = ImageDigest::parse("sha256:feedface").unwrap();
        let offline_runner = CountingRunner::unavailable();
        let cached_entry = cache
            .ensure(reference.clone(), &offline_runner)
            .expect("ingested tag should resolve without buildah");

        assert_eq!(entry.digest, digest);
        assert_eq!(cached_entry.digest, digest);
        assert_eq!(offline_runner.calls(), 0);
        assert_eq!(entry.compatibility, ImageCompatibility::Agentbox);
        assert_eq!(
            fs::read_to_string(entry.rootfs.join("etc/os-release")).expect("rootfs should copy"),
            "NAME=agentbox\n"
        );
        assert_eq!(
            commands.calls(),
            vec![
                vec!["--version"],
                vec!["from", "ghcr.io/example/agentbox:dev"],
                vec![
                    "inspect",
                    "--format",
                    "{{.FromImageDigest}}",
                    "agentbox-working-container"
                ],
                vec!["mount", "agentbox-working-container"],
                vec!["umount", "agentbox-working-container"],
                vec!["rm", "agentbox-working-container"],
            ]
        );
    }

    #[test]
    fn digest_pinned_cache_miss_rejects_buildah_digest_mismatch_without_ref_write() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let mount_root = temp.path().join("buildah-mount");
        write_guest_init(&mount_root);
        let reference = ImageReference::from_cli(Some("ghcr.io/example/agentbox@sha256:expected"));
        let commands = FakeBuildahCommands::new(mount_root)
            .with_reference("ghcr.io/example/agentbox@sha256:expected")
            .with_digest_output("sha256:actual\n");
        let cache = ImageCache::new(temp.path().join("images"));
        let runner = BuildahCommandIngestor::new(&commands);

        let error = cache
            .ensure(reference.clone(), &runner)
            .expect_err("digest mismatch should fail before cache writes");

        assert!(format!("{error:#}").contains("requested 'sha256:expected'"));
        assert!(
            cache
                .lookup_ref_digest(&reference)
                .expect("ref lookup should work")
                .is_none()
        );
        assert!(
            !cache
                .entry_path(&ImageDigest::parse("sha256:actual").unwrap())
                .exists()
        );
        assert_eq!(
            commands.calls(),
            vec![
                vec!["--version"],
                vec!["from", "ghcr.io/example/agentbox@sha256:expected"],
                vec![
                    "inspect",
                    "--format",
                    "{{.FromImageDigest}}",
                    "agentbox-working-container"
                ],
                vec!["rm", "agentbox-working-container"],
            ]
        );
    }

    #[test]
    fn empty_buildah_digest_fails_before_cache_or_ref_write() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let mount_root = temp.path().join("buildah-mount");
        write_guest_init(&mount_root);
        let reference = ImageReference::from_cli(Some("ghcr.io/example/agentbox:dev"));
        let commands = FakeBuildahCommands::new(mount_root).with_digest_output("\n");
        let cache = ImageCache::new(temp.path().join("images"));
        let runner = BuildahCommandIngestor::new(&commands);

        let error = cache
            .ensure(reference.clone(), &runner)
            .expect_err("empty buildah digest should fail ingestion");

        assert!(format!("{error:#}").contains("did not return an image digest"));
        assert!(
            cache
                .lookup_ref_digest(&reference)
                .expect("ref lookup should work")
                .is_none()
        );
        assert!(!cache.root.join("blobs").exists());
        assert_eq!(
            commands.calls().last().expect("last command should exist"),
            &vec!["rm".to_owned(), "agentbox-working-container".to_owned()]
        );
    }

    #[test]
    fn failed_compatibility_validation_removes_staging_and_ref_metadata() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let mount_root = temp.path().join("buildah-mount");
        fs::create_dir_all(mount_root.join("etc")).expect("mount root should be created");
        fs::write(mount_root.join("etc/os-release"), "NAME=plain\n")
            .expect("rootfs content should be written");
        let reference = ImageReference::from_cli(Some("ghcr.io/example/plain:dev"));
        let commands =
            FakeBuildahCommands::new(mount_root).with_reference("ghcr.io/example/plain:dev");
        let cache = ImageCache::new(temp.path().join("images"));
        let runner = BuildahCommandIngestor::new(&commands);

        let error = cache
            .ensure(reference.clone(), &runner)
            .expect_err("missing guest-init should fail ingestion");

        assert!(format!("{error:#}").contains("not agentbox-compatible"));
        assert!(
            cache
                .lookup_ref_digest(&reference)
                .expect("ref lookup should work")
                .is_none()
        );
        let algorithm_dir = cache.root.join("blobs/sha256");
        let remaining_entries = if algorithm_dir.exists() {
            fs::read_dir(&algorithm_dir)
                .expect("algorithm dir should be readable")
                .count()
        } else {
            0
        };
        assert_eq!(remaining_entries, 0);
        assert_eq!(
            commands.calls().last().expect("last command should exist"),
            &vec!["rm".to_owned(), "agentbox-working-container".to_owned()]
        );
        assert!(commands.calls().contains(&vec![
            "umount".to_owned(),
            "agentbox-working-container".to_owned()
        ]));
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
