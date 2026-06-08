//! Workspace-scoped host kernel-overlayfs ownership for guest `/nix`.
//!
//! Session code acquires the workspace lease and serializes the intent; the VM
//! worker materializes the mount in the namespace that will also prepare the
//! libkrun root graft.

use anyhow::{Context, Result, anyhow, bail};
use std::fs::{self, File, OpenOptions};
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::runtime::launch::config::HostNixOverlay;
use crate::runtime::session::rootfs::task::TaskRootfsHandle;

const OVERLAY_DIR: &str = "nix-overlay";
const UPPER_DIR: &str = "upper";
const WORK_DIR: &str = "work";
const MERGED_DIR: &str = "merged";
const LEASE_LOCK: &str = "lease.lock";
const LEASE_STATE: &str = "lease.state";
const CACHE_ROOTFS_DIR: &str = "rootfs";
const NIX_DIR: &str = "nix";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NixOverlayPaths {
    pub(crate) root: PathBuf,
    pub(crate) upperdir: PathBuf,
    pub(crate) workdir: PathBuf,
    pub(crate) mergeddir: PathBuf,
    pub(crate) lease_lock: PathBuf,
    pub(crate) lease_state: PathBuf,
}

impl NixOverlayPaths {
    pub(crate) fn new(workspace_state_root: &Path) -> Self {
        let root = workspace_state_root.join(OVERLAY_DIR);
        Self {
            upperdir: root.join(UPPER_DIR),
            workdir: root.join(WORK_DIR),
            mergeddir: root.join(MERGED_DIR),
            lease_lock: root.join(LEASE_LOCK),
            lease_state: root.join(LEASE_STATE),
            root,
        }
    }
}

pub(crate) struct NixOverlayLease {
    intent: HostNixOverlay,
    _lock_file: File,
}

impl NixOverlayLease {
    pub(crate) fn acquire(workspace_state_root: &Path, handle: &TaskRootfsHandle) -> Result<Self> {
        let paths = NixOverlayPaths::new(workspace_state_root);
        fs::create_dir_all(&paths.root).with_context(|| {
            format!(
                "failed to create loftd host /nix overlay root '{}'",
                paths.root.display()
            )
        })?;
        let lock_file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&paths.lease_lock)
            .with_context(|| {
                format!(
                    "failed to open loftd host /nix overlay lease '{}'",
                    paths.lease_lock.display()
                )
            })?;
        lock_exclusive(&lock_file).with_context(|| {
            format!(
                "failed to acquire loftd host /nix overlay lease '{}'",
                paths.lease_lock.display()
            )
        })?;

        let intent = intent_from_task_rootfs(&paths, handle)?;
        validate_existing_state(&paths.lease_state, &intent)?;
        write_state(&paths.lease_state, &intent)?;
        Ok(Self {
            intent,
            _lock_file: lock_file,
        })
    }

    pub(crate) fn intent(&self) -> &HostNixOverlay {
        &self.intent
    }
}

pub(crate) fn materialize_in_worker(intent: &HostNixOverlay) -> Result<NixOverlayWorkerMount> {
    materialize_in_worker_with(intent, &HostNixOverlayCommands)
}

fn materialize_in_worker_with(
    intent: &HostNixOverlay,
    commands: &impl NixOverlayCommands,
) -> Result<NixOverlayWorkerMount> {
    prepare_and_mount(intent, commands)?;
    Ok(NixOverlayWorkerMount {
        intent: intent.clone(),
        mounted: true,
    })
}

fn prepare_and_mount(intent: &HostNixOverlay, commands: &impl NixOverlayCommands) -> Result<()> {
    validate_absolute_intent(intent)?;
    commands.create_dir_all(&intent.upperdir)?;
    commands.create_dir_all(&intent.workdir)?;
    commands.create_dir_all(&intent.mergeddir)?;
    commands.mount_overlay(intent)
}

#[cfg(test)]
fn unmount_with(intent: &HostNixOverlay, commands: &impl NixOverlayCommands) -> Result<()> {
    commands.unmount_overlay(intent)
}

pub(crate) struct NixOverlayWorkerMount {
    intent: HostNixOverlay,
    mounted: bool,
}

impl NixOverlayWorkerMount {
    pub(crate) fn unmount(mut self) -> Result<()> {
        if self.mounted {
            HostNixOverlayCommands.unmount_overlay(&self.intent)?;
            self.mounted = false;
        }
        Ok(())
    }
}

impl Drop for NixOverlayWorkerMount {
    fn drop(&mut self) {
        if !self.mounted {
            return;
        }
        if let Err(err) = HostNixOverlayCommands.unmount_overlay(&self.intent) {
            eprintln!(
                "loftd: best-effort host /nix overlay unmount failed for '{}': {err:#}",
                self.intent.mergeddir.display()
            );
        }
    }
}

trait NixOverlayCommands {
    fn create_dir_all(&self, path: &Path) -> Result<()>;
    fn mount_overlay(&self, intent: &HostNixOverlay) -> Result<()>;
    fn unmount_overlay(&self, intent: &HostNixOverlay) -> Result<()>;
}

#[derive(Debug, Clone, Copy)]
struct HostNixOverlayCommands;

impl NixOverlayCommands for HostNixOverlayCommands {
    fn create_dir_all(&self, path: &Path) -> Result<()> {
        fs::create_dir_all(path).with_context(|| {
            format!(
                "failed to create loftd host /nix overlay path '{}'",
                path.display()
            )
        })
    }

    fn mount_overlay(&self, intent: &HostNixOverlay) -> Result<()> {
        let options = overlay_options(intent);
        let status = Command::new("mount")
            .args(["-t", "overlay", "overlay", "-o", &options])
            .arg(&intent.mergeddir)
            .status()
            .with_context(|| {
                format!(
                    "failed to run loftd host /nix overlay mount for '{}'",
                    intent.mergeddir.display()
                )
            })?;
        if !status.success() {
            bail!(
                "loftd host /nix overlay mount lowerdir='{}' upperdir='{}' workdir='{}' merged='{}' failed with {status}",
                intent.lowerdir.display(),
                intent.upperdir.display(),
                intent.workdir.display(),
                intent.mergeddir.display()
            );
        }
        Ok(())
    }

    fn unmount_overlay(&self, intent: &HostNixOverlay) -> Result<()> {
        let status = Command::new("umount")
            .arg(&intent.mergeddir)
            .status()
            .with_context(|| {
                format!(
                    "failed to run loftd host /nix overlay unmount for '{}'",
                    intent.mergeddir.display()
                )
            })?;
        if !status.success() {
            bail!(
                "loftd host /nix overlay unmount '{}' failed with {status}; retaining lease/state for diagnostics",
                intent.mergeddir.display()
            );
        }
        Ok(())
    }
}

fn intent_from_task_rootfs(
    paths: &NixOverlayPaths,
    handle: &TaskRootfsHandle,
) -> Result<HostNixOverlay> {
    let cache_profile = handle.cache_profile();
    let cache_path = cache_profile.cache_path.as_ref().ok_or_else(|| {
        anyhow!(
            "loftd host /nix overlay requires a digest-keyed image cache lowerdir; selected image '{}' was not cached: {}",
            handle.selected_image_reference(),
            cache_profile
                .uncached_reason
                .as_deref()
                .unwrap_or("missing-cache-profile")
        )
    })?;
    let digest_key = cache_profile.digest_key.clone().ok_or_else(|| {
        anyhow!(
            "loftd host /nix overlay requires selected image cache digest key for '{}'",
            handle.selected_image_reference()
        )
    })?;
    let image_digest = handle.image_digest().ok_or_else(|| {
        anyhow!(
            "loftd host /nix overlay requires selected image digest for '{}'",
            handle.selected_image_reference()
        )
    })?;
    let lowerdir = cache_path.join(CACHE_ROOTFS_DIR).join(NIX_DIR);
    if !lowerdir.is_dir() {
        bail!(
            "loftd host /nix overlay lowerdir '{}' is missing; selected image cache entry for '{}' does not contain /nix",
            lowerdir.display(),
            handle.selected_image_reference()
        );
    }
    Ok(HostNixOverlay {
        selected_reference: handle.selected_image_reference().to_owned(),
        image_digest: image_digest.to_owned(),
        digest_key,
        lowerdir,
        upperdir: paths.upperdir.clone(),
        workdir: paths.workdir.clone(),
        mergeddir: paths.mergeddir.clone(),
    })
}

fn validate_existing_state(path: &Path, intent: &HostNixOverlay) -> Result<()> {
    let Ok(text) = fs::read_to_string(path) else {
        return Ok(());
    };
    let state = parse_state(&text);
    let prior_digest = state.get("image_digest").map(String::as_str);
    let prior_lowerdir = state.get("lowerdir").map(String::as_str);
    if prior_digest == Some(intent.image_digest.as_str())
        && prior_lowerdir == Some(&intent.lowerdir.display().to_string())
    {
        return Ok(());
    }
    bail!(
        "loftd host /nix overlay state '{}' belongs to image digest {:?} lowerdir {:?}, refusing to reuse with digest '{}' lowerdir '{}'; reset the workspace nix-overlay state before switching images",
        path.display(),
        prior_digest,
        prior_lowerdir,
        intent.image_digest,
        intent.lowerdir.display()
    )
}

fn write_state(path: &Path, intent: &HostNixOverlay) -> Result<()> {
    let state = format!(
        "selected_reference={}\nimage_digest={}\ndigest_key={}\nlowerdir={}\nupperdir={}\nworkdir={}\nmergeddir={}\n",
        intent.selected_reference,
        intent.image_digest,
        intent.digest_key,
        intent.lowerdir.display(),
        intent.upperdir.display(),
        intent.workdir.display(),
        intent.mergeddir.display()
    );
    fs::write(path, state).with_context(|| {
        format!(
            "failed to write loftd host /nix overlay state '{}'",
            path.display()
        )
    })
}

fn parse_state(text: &str) -> std::collections::BTreeMap<String, String> {
    text.lines()
        .filter_map(|line| line.split_once('='))
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .collect()
}

fn lock_exclusive(file: &File) -> Result<()> {
    let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) };
    if rc == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error()).context("flock failed")
    }
}

fn validate_absolute_intent(intent: &HostNixOverlay) -> Result<()> {
    for (name, path) in [
        ("lowerdir", &intent.lowerdir),
        ("upperdir", &intent.upperdir),
        ("workdir", &intent.workdir),
        ("mergeddir", &intent.mergeddir),
    ] {
        if !path.is_absolute() {
            bail!(
                "loftd host /nix overlay {name} '{}' must be absolute",
                path.display()
            );
        }
    }
    Ok(())
}

fn overlay_options(intent: &HostNixOverlay) -> String {
    format!(
        "lowerdir={},upperdir={},workdir={}",
        intent.lowerdir.display(),
        intent.upperdir.display(),
        intent.workdir.display()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::session::rootfs::image_source::{
        ImageSourceCacheProfile, ImageSourceCacheStatus, OciProcessConfig,
    };
    use crate::runtime::session::rootfs::task::{TaskRootfsHandle, TaskRootfsHandleTestSpec};
    use crate::task_rootfs::TaskRootfsBackend;
    use std::cell::RefCell;

    #[test]
    fn overlay_paths_are_workspace_scoped() {
        let a = NixOverlayPaths::new(Path::new("/state/loftd/workspace-a"));
        let b = NixOverlayPaths::new(Path::new("/state/loftd/workspace-b"));

        assert_eq!(
            a.upperdir,
            Path::new("/state/loftd/workspace-a/nix-overlay/upper")
        );
        assert_eq!(
            a.workdir,
            Path::new("/state/loftd/workspace-a/nix-overlay/work")
        );
        assert_eq!(
            a.mergeddir,
            Path::new("/state/loftd/workspace-a/nix-overlay/merged")
        );
        assert_eq!(
            a.lease_lock,
            Path::new("/state/loftd/workspace-a/nix-overlay/lease.lock")
        );
        assert_ne!(a.upperdir, b.upperdir);
        assert_ne!(a.mergeddir, b.mergeddir);
    }

    #[test]
    fn intent_uses_digest_keyed_image_cache_nix_lowerdir() {
        let temp = tempfile::tempdir().expect("tempdir");
        let cache_entry = temp
            .path()
            .join("image-cache/btrfs-snapshots/sha256-deadbeef");
        fs::create_dir_all(cache_entry.join("rootfs/nix")).expect("cache lowerdir");
        let paths = NixOverlayPaths::new(temp.path().join("state/workspace").as_path());
        let handle = task_handle(
            cache_entry.clone(),
            Some("sha256:deadbeef"),
            Some("sha256-deadbeef"),
        );

        let intent = intent_from_task_rootfs(&paths, &handle).expect("intent");

        assert_eq!(intent.lowerdir, cache_entry.join("rootfs/nix"));
        assert_eq!(intent.image_digest, "sha256:deadbeef");
        assert_eq!(intent.digest_key, "sha256-deadbeef");
        assert_eq!(intent.upperdir, paths.upperdir);
        assert_eq!(intent.mergeddir, paths.mergeddir);
    }

    #[test]
    fn missing_cache_nix_lowerdir_fails_clearly() {
        let temp = tempfile::tempdir().expect("tempdir");
        let cache_entry = temp
            .path()
            .join("image-cache/btrfs-snapshots/sha256-deadbeef");
        fs::create_dir_all(cache_entry.join("rootfs")).expect("cache rootfs");
        let paths = NixOverlayPaths::new(temp.path());
        let handle = task_handle(
            cache_entry,
            Some("sha256:deadbeef"),
            Some("sha256-deadbeef"),
        );

        let err = intent_from_task_rootfs(&paths, &handle).expect_err("missing /nix fails");

        assert!(format!("{err:#}").contains("does not contain /nix"));
    }

    #[test]
    fn existing_state_for_different_digest_is_refused() {
        let temp = tempfile::tempdir().expect("tempdir");
        let state = temp.path().join("lease.state");
        fs::write(&state, "image_digest=sha256:old\nlowerdir=/old/nix\n").expect("state");
        let intent = HostNixOverlay {
            selected_reference: "localhost/loftd:latest".to_owned(),
            image_digest: "sha256:new".to_owned(),
            digest_key: "sha256-new".to_owned(),
            lowerdir: PathBuf::from("/new/nix"),
            upperdir: PathBuf::from("/state/nix-overlay/upper"),
            workdir: PathBuf::from("/state/nix-overlay/work"),
            mergeddir: PathBuf::from("/state/nix-overlay/merged"),
        };

        let err = validate_existing_state(&state, &intent).expect_err("mismatch refused");

        assert!(format!("{err:#}").contains("refusing to reuse"));
    }

    #[test]
    fn worker_mount_creates_dirs_then_mounts_overlay() {
        let runner = FakeOverlayCommands::default();
        let intent = absolute_intent();

        prepare_and_mount(&intent, &runner).expect("mount");

        assert_eq!(
            runner.calls.borrow().as_slice(),
            [
                "mkdir:/state/nix-overlay/upper",
                "mkdir:/state/nix-overlay/work",
                "mkdir:/state/nix-overlay/merged",
                "mount:lowerdir=/cache/rootfs/nix,upperdir=/state/nix-overlay/upper,workdir=/state/nix-overlay/work:/state/nix-overlay/merged",
            ]
        );
    }

    #[test]
    fn worker_unmount_records_namespace_visible_merged_target() {
        let runner = FakeOverlayCommands::default();
        let intent = absolute_intent();
        prepare_and_mount(&intent, &runner).expect("mount");
        unmount_with(&intent, &runner).expect("unmount");

        assert!(
            runner
                .calls
                .borrow()
                .contains(&"umount:/state/nix-overlay/merged".to_owned())
        );
    }

    fn task_handle(
        cache_path: PathBuf,
        image_digest: Option<&str>,
        digest_key: Option<&str>,
    ) -> TaskRootfsHandle {
        TaskRootfsHandle::new_for_test(TaskRootfsHandleTestSpec {
            task_id: "task".to_owned(),
            task_dir: PathBuf::from("/state/tasks/task"),
            rootfs_path: PathBuf::from("/state/tasks/task/rootfs-btrfs-snapshot"),
            backend: TaskRootfsBackend::BtrfsSnapshot,
            selected_image_reference: "localhost/loftd:latest".to_owned(),
            image_digest: image_digest.map(str::to_owned),
            process_config: OciProcessConfig::default(),
            cache_profile: ImageSourceCacheProfile {
                status: ImageSourceCacheStatus::Hit,
                digest_key: digest_key.map(str::to_owned),
                cache_path: Some(cache_path),
                uncached_reason: None,
            },
            preserve_debug: false,
        })
    }

    fn absolute_intent() -> HostNixOverlay {
        HostNixOverlay {
            selected_reference: "localhost/loftd:latest".to_owned(),
            image_digest: "sha256:deadbeef".to_owned(),
            digest_key: "sha256-deadbeef".to_owned(),
            lowerdir: PathBuf::from("/cache/rootfs/nix"),
            upperdir: PathBuf::from("/state/nix-overlay/upper"),
            workdir: PathBuf::from("/state/nix-overlay/work"),
            mergeddir: PathBuf::from("/state/nix-overlay/merged"),
        }
    }

    #[derive(Default)]
    struct FakeOverlayCommands {
        calls: RefCell<Vec<String>>,
    }

    impl NixOverlayCommands for FakeOverlayCommands {
        fn create_dir_all(&self, path: &Path) -> Result<()> {
            self.calls
                .borrow_mut()
                .push(format!("mkdir:{}", path.display()));
            Ok(())
        }

        fn mount_overlay(&self, intent: &HostNixOverlay) -> Result<()> {
            self.calls.borrow_mut().push(format!(
                "mount:{}:{}",
                overlay_options(intent),
                intent.mergeddir.display()
            ));
            Ok(())
        }

        fn unmount_overlay(&self, intent: &HostNixOverlay) -> Result<()> {
            self.calls
                .borrow_mut()
                .push(format!("umount:{}", intent.mergeddir.display()));
            Ok(())
        }
    }
}
