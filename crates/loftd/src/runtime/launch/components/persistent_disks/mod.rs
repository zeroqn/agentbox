//! Persistent launch disk contributors.
//!
//! Host-overlay `/nix` mode contributes `/nix` env plus the selected persistent
//! container-store carrier.

use anyhow::Result;
use std::path::Path;

use crate::runtime::launch::config::DiskAttachment;
use raw_btrfs::{HostRawImageCommandRunner, RawBtrfsDisk};

mod containers;
mod nix;
pub(crate) mod raw_btrfs;

pub(crate) fn grow_container_store(
    state_root: &Path,
    target_size_bytes: u64,
) -> Result<RawBtrfsDisk> {
    containers::grow_with_runner(state_root, target_size_bytes, &HostRawImageCommandRunner)
}

pub(crate) fn recreate_container_store(state_root: &Path) -> Result<RawBtrfsDisk> {
    containers::recreate_with_runner(state_root, &HostRawImageCommandRunner)
}

pub(crate) fn container_store_attachment(disk: &RawBtrfsDisk) -> DiskAttachment {
    containers::attachment(disk)
}

pub(crate) fn container_store_raw_disk_env_pairs(disk: &RawBtrfsDisk) -> [(String, String); 4] {
    containers::raw_disk_env_pairs(disk)
}

pub(crate) fn container_store_default_size_bytes() -> u64 {
    containers::default_size_bytes()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PersistentDisks {
    container_store: RawBtrfsDisk,
}

impl PersistentDisks {
    pub(crate) fn attachments(&self) -> Vec<DiskAttachment> {
        vec![containers::attachment(&self.container_store)]
    }

    pub(crate) fn env_pairs(&self) -> Vec<(String, String)> {
        let mut env = nix::host_overlay_env_pairs().to_vec();
        env.extend(containers::raw_disk_env_pairs(&self.container_store));
        env
    }
}

pub(crate) trait PersistentDiskPreparer {
    fn prepare(&self, state_root: &Path) -> Result<PersistentDisks>;
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct HostPersistentDiskPreparer;

impl PersistentDiskPreparer for HostPersistentDiskPreparer {
    fn prepare(&self, state_root: &Path) -> Result<PersistentDisks> {
        prepare_with_runner(state_root, &HostRawImageCommandRunner)
    }
}

fn prepare_with_runner(
    state_root: &Path,
    runner: &impl raw_btrfs::RawImageCommandRunner,
) -> Result<PersistentDisks> {
    let container_store = containers::prepare_with_runner(state_root, runner)?;
    Ok(PersistentDisks { container_store })
}

#[cfg(test)]
mod tests {
    use super::raw_btrfs::{RawBtrfsDiskStatus, test_support::FakeRunner};
    use super::*;
    use std::fs::File;

    #[test]
    fn prepares_workspace_scoped_container_store_raw_btrfs_disk_only() {
        let temp = tempfile::tempdir().expect("tempdir");
        let runner = FakeRunner::default();

        let disks = prepare_with_runner(temp.path(), &runner).expect("disks should prepare");

        assert!(!temp.path().join(nix::FILE_NAME).exists());
        let container_disk = disks.container_store;
        assert_eq!(container_disk.path, temp.path().join(containers::FILE_NAME));
        assert_eq!(container_disk.id, containers::ID);
        assert_eq!(container_disk.label, containers::LABEL);
        assert_eq!(runner.mkfs_call_count(), 1);
    }

    #[test]
    fn reuses_existing_valid_disks_without_formatting() {
        let temp = tempfile::tempdir().expect("tempdir");
        let file = File::create(temp.path().join(containers::FILE_NAME)).expect("disk file");
        file.set_len(containers::SIZE_BYTES).expect("disk size");
        let runner = FakeRunner::with_probe("btrfs");

        let disks = prepare_with_runner(temp.path(), &runner).expect("disks should prepare");

        let disk = disks.container_store;
        assert_eq!(disk.status, RawBtrfsDiskStatus::Reused);
        assert_eq!(runner.mkfs_call_count(), 0);
    }

    #[test]
    fn disk_validation_failure_is_classified_as_loftd_persistent_cache() {
        let temp = tempfile::tempdir().expect("tempdir");
        let file = File::create(temp.path().join(containers::FILE_NAME)).expect("disk file");
        file.set_len(containers::SIZE_BYTES).expect("disk size");
        let runner = FakeRunner::default();
        runner.push_probe_error("blkid exploded");

        let err = prepare_with_runner(temp.path(), &runner).expect_err("disk prep should fail");

        assert!(format!("{err:#}").contains("failed to prepare loftd persistent container-store"));
        assert!(format!("{err:#}").contains("blkid exploded"));
    }

    #[test]
    fn persistent_disk_owners_preserve_attachments() {
        let temp = tempfile::tempdir().expect("tempdir");
        let runner = FakeRunner::default();
        let disks = prepare_with_runner(temp.path(), &runner).expect("disks should prepare");

        let attachments = disks.attachments();

        assert_eq!(attachments.len(), 1);
        assert_eq!(attachments[0].id, containers::ID);
        assert_eq!(attachments[0].path, temp.path().join(containers::FILE_NAME));
        assert!(!attachments[0].read_only);
    }

    #[test]
    fn persistent_disk_owners_preserve_guest_env() {
        let temp = tempfile::tempdir().expect("tempdir");
        let runner = FakeRunner::default();
        let disks = prepare_with_runner(temp.path(), &runner).expect("disks should prepare");

        let env = disks.env_pairs();

        assert_eq!(
            env,
            vec![
                ("LOFTD_NIX_OVERLAY".to_owned(), "1".to_owned()),
                ("LOFTD_NIX_HOST_OVERLAY".to_owned(), "1".to_owned()),
                ("LOFTD_CONTAINERS_STORAGE".to_owned(), "1".to_owned()),
                ("LOFTD_CONTAINERS_STORE".to_owned(), "raw-disk".to_owned()),
                (
                    "LOFTD_CONTAINERS_DISK_ID".to_owned(),
                    "loftd-containers".to_owned(),
                ),
                (
                    "LOFTD_CONTAINERS_DISK_LABEL".to_owned(),
                    "LOFTD_CONTAINERS".to_owned(),
                ),
            ]
        );
        assert!(!env.iter().any(|(key, _)| key == "LOFTD_NIX_DISK_ID"));
        assert!(!env.iter().any(|(key, _)| key == "LOFTD_NIX_DISK_LABEL"));
        assert!(env.iter().all(|(key, _)| !key.starts_with("AGENTBOX_")));
    }
}
