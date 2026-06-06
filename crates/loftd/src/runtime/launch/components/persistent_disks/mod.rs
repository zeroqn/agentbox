//! Persistent launch disk contributors.
//!
//! The aggregate preserves the existing Nix-then-containers ordering. Owner
//! files below this module do not introduce new disk policy or validation.

use anyhow::Result;
use std::path::Path;

use crate::runtime::launch::config::DiskAttachment;
use raw_btrfs::{HostRawImageCommandRunner, RawBtrfsDisk};

mod containers;
mod nix;
pub(crate) mod raw_btrfs;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PersistentDisks {
    pub(crate) nix: RawBtrfsDisk,
    pub(crate) containers: RawBtrfsDisk,
}

impl PersistentDisks {
    pub(crate) fn attachments(&self) -> Vec<DiskAttachment> {
        vec![
            nix::attachment(&self.nix),
            containers::attachment(&self.containers),
        ]
    }

    pub(crate) fn env_pairs(&self) -> Vec<(String, String)> {
        nix::env_pairs(&self.nix)
            .into_iter()
            .chain(containers::env_pairs(&self.containers))
            .collect()
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
    let nix = nix::prepare_with_runner(state_root, runner)?;
    let containers = containers::prepare_with_runner(state_root, runner)?;
    Ok(PersistentDisks { nix, containers })
}

#[cfg(test)]
mod tests {
    use super::raw_btrfs::{RawBtrfsDiskStatus, test_support::FakeRunner};
    use super::*;
    use std::fs::File;

    #[test]
    fn prepares_workspace_scoped_nix_and_container_store_raw_btrfs_disks() {
        let temp = tempfile::tempdir().expect("tempdir");
        let runner = FakeRunner::default();

        let disks = prepare_with_runner(temp.path(), &runner).expect("disks should prepare");

        assert_eq!(disks.nix.status, RawBtrfsDiskStatus::Created);
        assert_eq!(disks.nix.path, temp.path().join(nix::FILE_NAME));
        assert_eq!(disks.nix.id, nix::ID);
        assert_eq!(disks.nix.label, nix::LABEL);
        assert_eq!(
            disks.containers.path,
            temp.path().join(containers::FILE_NAME)
        );
        assert_eq!(disks.containers.id, containers::ID);
        assert_eq!(disks.containers.label, containers::LABEL);
        assert_eq!(runner.mkfs_call_count(), 2);
    }

    #[test]
    fn reuses_existing_valid_disks_without_formatting() {
        let temp = tempfile::tempdir().expect("tempdir");
        for (name, size_bytes) in [
            (nix::FILE_NAME, nix::SIZE_BYTES),
            (containers::FILE_NAME, containers::SIZE_BYTES),
        ] {
            let file = File::create(temp.path().join(name)).expect("disk file");
            file.set_len(size_bytes).expect("disk size");
        }
        let runner = FakeRunner::with_probe("btrfs");
        runner.push_probe("btrfs");

        let disks = prepare_with_runner(temp.path(), &runner).expect("disks should prepare");

        assert_eq!(disks.nix.status, RawBtrfsDiskStatus::Reused);
        assert_eq!(disks.containers.status, RawBtrfsDiskStatus::Reused);
        assert_eq!(runner.mkfs_call_count(), 0);
    }

    #[test]
    fn disk_validation_failure_is_classified_as_loftd_persistent_cache() {
        let temp = tempfile::tempdir().expect("tempdir");
        let file = File::create(temp.path().join(nix::FILE_NAME)).expect("disk file");
        file.set_len(nix::SIZE_BYTES).expect("disk size");
        let runner = FakeRunner::default();
        runner.push_probe_error("blkid exploded");

        let err = prepare_with_runner(temp.path(), &runner).expect_err("disk prep should fail");

        assert!(format!("{err:#}").contains("failed to prepare loftd persistent /nix"));
        assert!(format!("{err:#}").contains("blkid exploded"));
    }

    #[test]
    fn persistent_disk_owners_preserve_attachments() {
        let temp = tempfile::tempdir().expect("tempdir");
        let runner = FakeRunner::default();
        let disks = prepare_with_runner(temp.path(), &runner).expect("disks should prepare");

        let attachments = disks.attachments();

        assert_eq!(attachments.len(), 2);
        assert_eq!(attachments[0].id, nix::ID);
        assert_eq!(attachments[0].path, temp.path().join(nix::FILE_NAME));
        assert!(!attachments[0].read_only);
        assert_eq!(attachments[1].id, containers::ID);
        assert_eq!(attachments[1].path, temp.path().join(containers::FILE_NAME));
        assert!(!attachments[1].read_only);
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
                ("LOFTD_NIX_DISK_ID".to_owned(), "loftd-nix".to_owned()),
                ("LOFTD_NIX_DISK_LABEL".to_owned(), "LOFTD_NIX".to_owned()),
                ("LOFTD_CONTAINERS_STORAGE".to_owned(), "1".to_owned()),
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
        assert!(env.iter().all(|(key, _)| !key.starts_with("AGENTBOX_")));
    }
}
