use anyhow::{Context, Result};
use std::path::Path;

use crate::runtime::launch_config::DiskAttachment;
use crate::runtime::raw_btrfs::{self, HostRawImageCommandRunner, RawBtrfsDisk, RawDiskSpec};

pub(crate) const LOFTD_NIX_DISK_FILE_NAME: &str = "loftd-nix.raw";
pub(crate) const LOFTD_CONTAINERS_DISK_FILE_NAME: &str = "loftd-containers.raw";
pub(crate) const LOFTD_NIX_DISK_ID: &str = "loftd-nix";
pub(crate) const LOFTD_NIX_DISK_LABEL: &str = "LOFTD_NIX";
pub(crate) const LOFTD_CONTAINERS_DISK_ID: &str = "loftd-containers";
pub(crate) const LOFTD_CONTAINERS_DISK_LABEL: &str = "LOFTD_CONTAINERS";
const LOFTD_DISK_SIZE_BYTES: u64 = 64 * 1024 * 1024 * 1024;

const LOFTD_NIX_DISK_SPEC: RawDiskSpec = RawDiskSpec {
    file_name: LOFTD_NIX_DISK_FILE_NAME,
    id: LOFTD_NIX_DISK_ID,
    label: LOFTD_NIX_DISK_LABEL,
    size_bytes: LOFTD_DISK_SIZE_BYTES,
    size_hint: "64G",
    diagnostic_name: "loftd persistent /nix dev cache disk",
};

const LOFTD_CONTAINERS_DISK_SPEC: RawDiskSpec = RawDiskSpec {
    file_name: LOFTD_CONTAINERS_DISK_FILE_NAME,
    id: LOFTD_CONTAINERS_DISK_ID,
    label: LOFTD_CONTAINERS_DISK_LABEL,
    size_bytes: LOFTD_DISK_SIZE_BYTES,
    size_hint: "64G",
    diagnostic_name: "loftd persistent container-store dev cache disk",
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PersistentDisks {
    pub(crate) nix: RawBtrfsDisk,
    pub(crate) containers: RawBtrfsDisk,
}

impl PersistentDisks {
    pub(crate) fn attachments(&self) -> Vec<DiskAttachment> {
        vec![
            DiskAttachment {
                id: self.nix.id.clone(),
                path: self.nix.path.clone(),
                read_only: false,
            },
            DiskAttachment {
                id: self.containers.id.clone(),
                path: self.containers.path.clone(),
                read_only: false,
            },
        ]
    }

    pub(crate) fn env_pairs(&self) -> Vec<(String, String)> {
        vec![
            ("LOFTD_NIX_OVERLAY".to_owned(), "1".to_owned()),
            ("LOFTD_NIX_DISK_ID".to_owned(), self.nix.id.clone()),
            ("LOFTD_NIX_DISK_LABEL".to_owned(), self.nix.label.clone()),
            ("LOFTD_CONTAINERS_STORAGE".to_owned(), "1".to_owned()),
            (
                "LOFTD_CONTAINERS_DISK_ID".to_owned(),
                self.containers.id.clone(),
            ),
            (
                "LOFTD_CONTAINERS_DISK_LABEL".to_owned(),
                self.containers.label.clone(),
            ),
        ]
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
    let nix = raw_btrfs::prepare_with_runner(state_root, &LOFTD_NIX_DISK_SPEC, runner)
        .context("failed to prepare loftd persistent /nix dev cache disk")?;
    let containers =
        raw_btrfs::prepare_with_runner(state_root, &LOFTD_CONTAINERS_DISK_SPEC, runner)
            .context("failed to prepare loftd persistent container-store dev cache disk")?;
    Ok(PersistentDisks { nix, containers })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::raw_btrfs::{RawBtrfsDiskStatus, test_support::FakeRunner};
    use std::fs::File;

    #[test]
    fn prepares_workspace_scoped_nix_and_container_store_raw_btrfs_disks() {
        let temp = tempfile::tempdir().expect("tempdir");
        let runner = FakeRunner::default();

        let disks = prepare_with_runner(temp.path(), &runner).expect("disks should prepare");

        assert_eq!(disks.nix.status, RawBtrfsDiskStatus::Created);
        assert_eq!(disks.nix.path, temp.path().join(LOFTD_NIX_DISK_FILE_NAME));
        assert_eq!(disks.nix.id, LOFTD_NIX_DISK_ID);
        assert_eq!(disks.nix.label, LOFTD_NIX_DISK_LABEL);
        assert_eq!(
            disks.containers.path,
            temp.path().join(LOFTD_CONTAINERS_DISK_FILE_NAME)
        );
        assert_eq!(disks.containers.id, LOFTD_CONTAINERS_DISK_ID);
        assert_eq!(disks.containers.label, LOFTD_CONTAINERS_DISK_LABEL);
        assert_eq!(runner.mkfs_call_count(), 2);
    }

    #[test]
    fn reuses_existing_valid_disks_without_formatting() {
        let temp = tempfile::tempdir().expect("tempdir");
        for name in [LOFTD_NIX_DISK_FILE_NAME, LOFTD_CONTAINERS_DISK_FILE_NAME] {
            let file = File::create(temp.path().join(name)).expect("disk file");
            file.set_len(LOFTD_DISK_SIZE_BYTES).expect("disk size");
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
        let file = File::create(temp.path().join(LOFTD_NIX_DISK_FILE_NAME)).expect("disk file");
        file.set_len(LOFTD_DISK_SIZE_BYTES).expect("disk size");
        let runner = FakeRunner::default();
        runner.push_probe_error("blkid exploded");

        let err = prepare_with_runner(temp.path(), &runner).expect_err("disk prep should fail");

        assert!(format!("{err:#}").contains("failed to prepare loftd persistent /nix"));
        assert!(format!("{err:#}").contains("blkid exploded"));
    }

    #[test]
    fn exports_only_loftd_guest_env_contract() {
        let temp = tempfile::tempdir().expect("tempdir");
        let runner = FakeRunner::default();
        let disks = prepare_with_runner(temp.path(), &runner).expect("disks should prepare");

        let env = disks.env_pairs();

        assert!(env.contains(&("LOFTD_NIX_OVERLAY".to_owned(), "1".to_owned())));
        assert!(env.contains(&("LOFTD_NIX_DISK_ID".to_owned(), "loftd-nix".to_owned())));
        assert!(env.contains(&("LOFTD_CONTAINERS_STORAGE".to_owned(), "1".to_owned())));
        assert!(env.contains(&(
            "LOFTD_CONTAINERS_DISK_ID".to_owned(),
            "loftd-containers".to_owned()
        )));
        assert!(env.iter().all(|(key, _)| !key.starts_with("AGENTBOX_")));
    }
}
