use anyhow::{Context, Result, anyhow};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::DEFAULT_IMAGE;
#[cfg(test)]
use crate::runtime::microvm::guest_init::find_agentbox_guest_init;
#[cfg(test)]
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
    fn run_unshare(&self, script: &str, args: &[&str]) -> Result<String>;
}

#[derive(Debug, Clone, Copy)]
struct HostBuildahCommandRunner;

impl BuildahCommandRunner for HostBuildahCommandRunner {
    fn run(&self, args: &[&str]) -> Result<String> {
        run_buildah(args)
    }

    fn run_unshare(&self, script: &str, args: &[&str]) -> Result<String> {
        let script_dir = std::env::temp_dir().join(format!(
            "agentbox-buildah-unshare-{}-{}",
            std::process::id(),
            monotonic_nanos()
        ));
        fs::create_dir_all(&script_dir).with_context(|| {
            format!(
                "failed to create temporary buildah unshare script directory '{}'",
                script_dir.display()
            )
        })?;
        let script_path = script_dir.join("ingest.sh");
        let write_result = (|| {
            fs::write(&script_path, script).with_context(|| {
                format!(
                    "failed to write buildah unshare script '{}'",
                    script_path.display()
                )
            })?;
            fs::set_permissions(&script_path, fs::Permissions::from_mode(0o700)).with_context(
                || {
                    format!(
                        "failed to make buildah unshare script executable '{}'",
                        script_path.display()
                    )
                },
            )?;

            let mut command_args = vec![
                "unshare".to_owned(),
                "sh".to_owned(),
                script_path.display().to_string(),
            ];
            command_args.extend(args.iter().map(|arg| (*arg).to_owned()));
            let borrowed = command_args.iter().map(String::as_str).collect::<Vec<_>>();
            run_buildah(&borrowed)
        })();
        let cleanup_result = fs::remove_dir_all(&script_dir).with_context(|| {
            format!(
                "failed to clean temporary buildah unshare script directory '{}'",
                script_dir.display()
            )
        });

        match (write_result, cleanup_result) {
            (Ok(output), Ok(())) => Ok(output),
            (Ok(_), Err(err)) => Err(err),
            (Err(err), Ok(())) => Err(err),
            (Err(err), Err(cleanup_err)) => Err(cleanup_err.context(format!(
                "buildah unshare script cleanup failed after command error: {err:#}"
            ))),
        }
    }
}

fn run_buildah(args: &[&str]) -> Result<String> {
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

    let expected_digest = reference
        .digest()
        .map(|digest| digest.as_str().to_owned())
        .unwrap_or_default();
    let cache_root = cache_root
        .to_str()
        .ok_or_else(|| anyhow!("microvm image cache path is not valid UTF-8"))?;
    let output = commands
        .run_unshare(
            buildah_ingestion_script(),
            &[reference.as_str(), cache_root, expected_digest.as_str()],
        )
        .with_context(|| {
            format!(
                "failed to ingest microvm image '{}' with rootless buildah unshare",
                reference.as_str()
            )
        })?;
    ImageDigest::parse(&trim_required(
        output,
        "buildah unshare ingestion did not return an image digest",
    )?)
}

fn trim_required(output: String, empty_message: &'static str) -> Result<String> {
    let trimmed = output.trim();
    if trimmed.is_empty() {
        anyhow::bail!(empty_message);
    }
    Ok(trimmed.to_owned())
}

fn buildah_ingestion_script() -> &'static str {
    r#"set -eu

image_ref=$1
cache_root=$2
expected_digest=$3

container_id=
mounted=0
staging_dir=

cleanup() {
  status=$?
  if [ "$mounted" = 1 ] && [ -n "$container_id" ]; then
    buildah umount "$container_id" >/dev/null 2>&1 || true
  fi
  if [ -n "$container_id" ]; then
    buildah rm "$container_id" >/dev/null 2>&1 || true
  fi
  if [ "$status" -ne 0 ] && [ -n "$staging_dir" ] && [ -d "$staging_dir" ]; then
    rm -rf "$staging_dir" >/dev/null 2>&1 || true
  fi
  exit "$status"
}
trap cleanup EXIT HUP INT TERM

container_id=$(buildah from "$image_ref")
digest=$(buildah inspect --format '{{.FromImageDigest}}' "$container_id")
if [ -z "$digest" ]; then
  echo "buildah inspect did not return an image digest" >&2
  exit 20
fi
if [ -n "$expected_digest" ] && [ "$digest" != "$expected_digest" ]; then
  echo "buildah resolved '$image_ref' to digest '$digest', but the image reference requested '$expected_digest'" >&2
  exit 21
fi

algorithm=${digest%%:*}
value=${digest#*:}
if [ "$algorithm" = "$digest" ] || [ -z "$algorithm" ] || [ -z "$value" ]; then
  echo "image digest '$digest' must use algorithm:value form" >&2
  exit 22
fi
case "$algorithm" in
  */*|*..*) echo "image digest '$digest' is not safe for cache paths" >&2; exit 23 ;;
esac
case "$value" in
  */*|*..*) echo "image digest '$digest' is not safe for cache paths" >&2; exit 23 ;;
esac

entry_dir=$cache_root/blobs/$algorithm/$value
if [ -d "$entry_dir/rootfs" ] && [ -e "$entry_dir/agentbox-compatible" ]; then
  printf '%s\n' "$digest"
  exit 0
fi
entry_parent=$(dirname "$entry_dir")
mkdir -p "$entry_parent"
staging_dir=$entry_parent/.$value.partial-$$-$(date +%s%N 2>/dev/null || date +%s)
rm -rf "$staging_dir"
mkdir -p "$staging_dir/rootfs"

mount_path=$(buildah mount "$container_id")
mounted=1
cp -a --reflink=auto "$mount_path"/. "$staging_dir/rootfs"/

store_dir=$staging_dir/rootfs/nix/store
matches=
if [ -d "$store_dir" ]; then
  matches=$(find "$store_dir" -mindepth 3 -maxdepth 3 -type f -path '*/bin/agentbox-guest-init' -perm /111 -print 2>/dev/null || true)
fi
match_count=$(printf '%s\n' "$matches" | sed '/^$/d' | wc -l | tr -d ' ')
if [ "$match_count" = 0 ]; then
  echo "buildah-ingested image digest '$digest' is not agentbox-compatible: no executable agentbox-guest-init found under /nix/store/*/bin" >&2
  exit 24
fi
if [ "$match_count" != 1 ]; then
  echo "buildah-ingested image digest '$digest' is ambiguous: found $match_count executable agentbox-guest-init binaries under /nix/store/*/bin" >&2
  exit 25
fi

printf 'agentbox\n' > "$staging_dir/agentbox-compatible"
if mv "$staging_dir" "$entry_dir"; then
  staging_dir=
  printf '%s\n' "$digest"
  exit 0
fi
if [ -d "$entry_dir/rootfs" ] && [ -e "$entry_dir/agentbox-compatible" ]; then
  rm -rf "$staging_dir"
  staging_dir=
  printf '%s\n' "$digest"
  exit 0
fi
echo "failed to finalize microvm image cache entry '$entry_dir'" >&2
exit 26
"#
}

#[cfg(test)]
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

fn cache_entry_path(cache_root: &Path, digest: &ImageDigest) -> PathBuf {
    let (algorithm, value) = digest.path_components();
    cache_root.join(BLOBS_DIR).join(algorithm).join(value)
}

#[cfg(test)]
fn staging_entry_path(entry_dir: &Path) -> PathBuf {
    let name = entry_dir
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("entry");
    let nanos = monotonic_nanos();
    entry_dir.with_file_name(format!(".{name}.partial-{}-{nanos}", std::process::id()))
}

fn monotonic_nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0)
}

#[cfg(test)]
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
        unshare_scripts: RefCell<Vec<String>>,
        mount_root: PathBuf,
        reference: String,
        digest_output: String,
    }

    impl FakeBuildahCommands {
        fn new(mount_root: PathBuf) -> Self {
            Self {
                calls: RefCell::new(Vec::new()),
                unshare_scripts: RefCell::new(Vec::new()),
                mount_root,
                reference: "ghcr.io/example/agentbox:dev".to_owned(),
                digest_output: "sha256:feedface\n".to_owned(),
            }
        }

        fn calls(&self) -> Vec<Vec<String>> {
            self.calls.borrow().clone()
        }

        fn unshare_scripts(&self) -> Vec<String> {
            self.unshare_scripts.borrow().clone()
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
            match args {
                ["--version"] => Ok("buildah version 1.42.0\n".to_owned()),
                other => anyhow::bail!("unexpected buildah args: {other:?}"),
            }
        }

        fn run_unshare(&self, script: &str, args: &[&str]) -> Result<String> {
            self.unshare_scripts.borrow_mut().push(script.to_owned());
            let mut call = vec!["unshare".to_owned(), "sh".to_owned(), "<script>".to_owned()];
            call.extend(args.iter().map(|arg| (*arg).to_owned()));
            self.calls.borrow_mut().push(call);

            let [reference, cache_root, expected_digest] = args else {
                anyhow::bail!("unexpected buildah unshare args: {args:?}");
            };
            if *reference != self.reference {
                anyhow::bail!("unexpected buildah reference: {reference}");
            }
            let digest = trim_required(
                self.digest_output.clone(),
                "buildah inspect did not return an image digest",
            )?;
            let digest = ImageDigest::parse(&digest)?;
            if !expected_digest.is_empty() && *expected_digest != digest.as_str() {
                anyhow::bail!(
                    "buildah resolved '{}' to digest '{}', but the image reference requested '{}'",
                    reference,
                    digest.as_str(),
                    expected_digest
                );
            }
            finalize_cache_entry(Path::new(cache_root), &digest, &self.mount_root)?;
            Ok(format!("{}\n", digest.as_str()))
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
                vec![
                    "unshare",
                    "sh",
                    "<script>",
                    "ghcr.io/example/agentbox:dev",
                    cache.root.to_str().expect("cache root should be utf8"),
                    ""
                ],
            ]
        );
        let script = commands
            .unshare_scripts()
            .pop()
            .expect("unshare script should be captured");
        assert!(script.contains("buildah from \"$image_ref\""));
        assert!(script.contains("buildah inspect --format '{{.FromImageDigest}}'"));
        assert!(script.contains("buildah mount \"$container_id\""));
        assert!(script.contains("cp -a --reflink=auto \"$mount_path\"/. \"$staging_dir/rootfs\"/"));
        assert!(script.contains("buildah umount \"$container_id\""));
        assert!(script.contains("buildah rm \"$container_id\""));
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
                vec![
                    "unshare",
                    "sh",
                    "<script>",
                    "ghcr.io/example/agentbox@sha256:expected",
                    cache.root.to_str().expect("cache root should be utf8"),
                    "sha256:expected"
                ],
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
        assert_eq!(commands.calls().len(), 2);
        assert_eq!(commands.calls()[0], vec!["--version"]);
        assert_eq!(commands.calls()[1][0..3], ["unshare", "sh", "<script>"]);
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
        assert_eq!(commands.calls().len(), 2);
        assert_eq!(commands.calls()[0], vec!["--version"]);
        assert_eq!(commands.calls()[1][0..3], ["unshare", "sh", "<script>"]);
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
