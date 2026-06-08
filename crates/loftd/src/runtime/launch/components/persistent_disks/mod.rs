//! Persistent launch disk contributors.
//!
//! Host-overlay `/nix` mode contributes `/nix` env plus the selected persistent
//! container-store carrier.

use anyhow::Result;
use std::path::Path;

use crate::cli::ContainerStoreBackend;
use crate::runtime::launch::config::DiskAttachment;
use raw_btrfs::{HostRawImageCommandRunner, RawBtrfsDisk};

mod containers;
mod nix;
pub(crate) mod raw_btrfs;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PersistentDisks {
    container_store: PersistentContainerStore,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PersistentContainerStore {
    Bind,
    RawDisk(RawBtrfsDisk),
}

impl PersistentDisks {
    pub(crate) fn attachments(&self) -> Vec<DiskAttachment> {
        match &self.container_store {
            PersistentContainerStore::Bind => Vec::new(),
            PersistentContainerStore::RawDisk(disk) => vec![containers::attachment(disk)],
        }
    }

    pub(crate) fn env_pairs(&self) -> Vec<(String, String)> {
        let mut env = nix::host_overlay_env_pairs().to_vec();
        match &self.container_store {
            PersistentContainerStore::Bind => {
                env.extend(containers::bind_env_pairs());
            }
            PersistentContainerStore::RawDisk(disk) => {
                env.extend(containers::raw_disk_env_pairs(disk));
            }
        }
        env
    }
}

pub(crate) trait PersistentDiskPreparer {
    fn prepare(
        &self,
        state_root: &Path,
        container_store_backend: ContainerStoreBackend,
    ) -> Result<PersistentDisks>;
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct HostPersistentDiskPreparer;

impl PersistentDiskPreparer for HostPersistentDiskPreparer {
    fn prepare(
        &self,
        state_root: &Path,
        container_store_backend: ContainerStoreBackend,
    ) -> Result<PersistentDisks> {
        prepare_with_runner(
            state_root,
            container_store_backend,
            &HostRawImageCommandRunner,
        )
    }
}

fn prepare_with_runner(
    state_root: &Path,
    container_store_backend: ContainerStoreBackend,
    runner: &impl raw_btrfs::RawImageCommandRunner,
) -> Result<PersistentDisks> {
    let container_store = match container_store_backend {
        ContainerStoreBackend::Bind => PersistentContainerStore::Bind,
        ContainerStoreBackend::RawDisk => {
            PersistentContainerStore::RawDisk(containers::prepare_with_runner(state_root, runner)?)
        }
    };
    Ok(PersistentDisks { container_store })
}

#[cfg(test)]
mod tests {
    use super::raw_btrfs::{RawBtrfsDiskStatus, test_support::FakeRunner};
    use super::*;
    use crate::cli::ContainerStoreBackend;
    use std::fs::File;

    #[test]
    fn prepares_workspace_scoped_container_store_raw_btrfs_disk_only() {
        let temp = tempfile::tempdir().expect("tempdir");
        let runner = FakeRunner::default();

        let disks = prepare_with_runner(temp.path(), ContainerStoreBackend::RawDisk, &runner)
            .expect("disks should prepare");

        assert!(!temp.path().join(nix::FILE_NAME).exists());
        let PersistentContainerStore::RawDisk(container_disk) = disks.container_store else {
            panic!("raw disk should be prepared");
        };
        assert_eq!(container_disk.path, temp.path().join(containers::FILE_NAME));
        assert_eq!(container_disk.id, containers::ID);
        assert_eq!(container_disk.label, containers::LABEL);
        assert_eq!(runner.mkfs_call_count(), 1);
    }

    #[test]
    fn bind_container_store_skips_raw_disk_preparation() {
        let temp = tempfile::tempdir().expect("tempdir");
        let runner = FakeRunner::default();

        let disks = prepare_with_runner(temp.path(), ContainerStoreBackend::Bind, &runner)
            .expect("bind store should prepare");

        assert_eq!(disks.container_store, PersistentContainerStore::Bind);
        assert_eq!(disks.attachments(), Vec::new());
        assert_eq!(runner.mkfs_call_count(), 0);
        assert!(!temp.path().join(containers::FILE_NAME).exists());
    }

    #[test]
    fn reuses_existing_valid_disks_without_formatting() {
        let temp = tempfile::tempdir().expect("tempdir");
        let file = File::create(temp.path().join(containers::FILE_NAME)).expect("disk file");
        file.set_len(containers::SIZE_BYTES).expect("disk size");
        let runner = FakeRunner::with_probe("btrfs");

        let disks = prepare_with_runner(temp.path(), ContainerStoreBackend::RawDisk, &runner)
            .expect("disks should prepare");

        let PersistentContainerStore::RawDisk(disk) = disks.container_store else {
            panic!("raw disk should exist");
        };
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

        let err = prepare_with_runner(temp.path(), ContainerStoreBackend::RawDisk, &runner)
            .expect_err("disk prep should fail");

        assert!(format!("{err:#}").contains("failed to prepare loftd persistent container-store"));
        assert!(format!("{err:#}").contains("blkid exploded"));
    }

    #[test]
    fn persistent_disk_owners_preserve_attachments() {
        let temp = tempfile::tempdir().expect("tempdir");
        let runner = FakeRunner::default();
        let disks = prepare_with_runner(temp.path(), ContainerStoreBackend::RawDisk, &runner)
            .expect("disks should prepare");

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
        let disks = prepare_with_runner(temp.path(), ContainerStoreBackend::RawDisk, &runner)
            .expect("disks should prepare");

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

    #[test]
    fn bind_container_store_preserves_nix_overlay_env_without_disk_env() {
        let temp = tempfile::tempdir().expect("tempdir");
        let runner = FakeRunner::default();
        let disks = prepare_with_runner(temp.path(), ContainerStoreBackend::Bind, &runner)
            .expect("bind store should prepare");

        let env = disks.env_pairs();

        assert_eq!(
            env,
            vec![
                ("LOFTD_NIX_OVERLAY".to_owned(), "1".to_owned()),
                ("LOFTD_NIX_HOST_OVERLAY".to_owned(), "1".to_owned()),
                ("LOFTD_CONTAINERS_STORAGE".to_owned(), "1".to_owned()),
                ("LOFTD_CONTAINERS_STORE".to_owned(), "bind".to_owned()),
            ]
        );
        assert!(!env.iter().any(|(key, _)| key == "LOFTD_CONTAINERS_DISK_ID"));
        assert!(
            !env.iter()
                .any(|(key, _)| key == "LOFTD_CONTAINERS_DISK_LABEL")
        );
    }
}
