use anyhow::{Context, Result};
use std::path::Path;

use crate::runtime::libkrun::raw_btrfs::{
    self, HostRawImageCommandRunner, RawBtrfsDisk, RawDiskSpec,
};
use crate::runtime::microvm::launch::MicrovmDiskAttachment;

pub(crate) const MICROVM_NIX_DISK_FILE_NAME: &str = "microvm-nix.raw";
pub(crate) const MICROVM_CONTAINERS_DISK_FILE_NAME: &str = "microvm-containers.raw";
pub(crate) const MICROVM_NIX_DISK_ID: &str = "agentbox-nix";
pub(crate) const MICROVM_NIX_DISK_LABEL: &str = "AGENTBOX_NIX";
pub(crate) const MICROVM_CONTAINERS_DISK_ID: &str = "agentbox-containers";
pub(crate) const MICROVM_CONTAINERS_DISK_LABEL: &str = "AGENTBOX_CONTAINERS";
const MICROVM_DISK_SIZE_BYTES: u64 = 64 * 1024 * 1024 * 1024;

const MICROVM_NIX_DISK_SPEC: RawDiskSpec = RawDiskSpec {
    file_name: MICROVM_NIX_DISK_FILE_NAME,
    id: MICROVM_NIX_DISK_ID,
    label: MICROVM_NIX_DISK_LABEL,
    size_bytes: MICROVM_DISK_SIZE_BYTES,
    size_hint: "64G",
    diagnostic_name: "microvm persistent /nix dev cache disk",
};

const MICROVM_CONTAINERS_DISK_SPEC: RawDiskSpec = RawDiskSpec {
    file_name: MICROVM_CONTAINERS_DISK_FILE_NAME,
    id: MICROVM_CONTAINERS_DISK_ID,
    label: MICROVM_CONTAINERS_DISK_LABEL,
    size_bytes: MICROVM_DISK_SIZE_BYTES,
    size_hint: "64G",
    diagnostic_name: "microvm persistent container-store dev cache disk",
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MicrovmPersistentDisks {
    pub(crate) nix: RawBtrfsDisk,
    pub(crate) containers: RawBtrfsDisk,
}

impl MicrovmPersistentDisks {
    pub(crate) fn attachments(&self) -> Vec<MicrovmDiskAttachment> {
        vec![
            MicrovmDiskAttachment {
                id: self.nix.id.clone(),
                path: self.nix.path.clone(),
                read_only: false,
            },
            MicrovmDiskAttachment {
                id: self.containers.id.clone(),
                path: self.containers.path.clone(),
                read_only: false,
            },
        ]
    }

    pub(crate) fn env_pairs(&self) -> Vec<(String, String)> {
        vec![
            ("AGENTBOX_LIBKRUN_NIX_OVERLAY".to_owned(), "1".to_owned()),
            (
                "AGENTBOX_LIBKRUN_NIX_DISK_ID".to_owned(),
                self.nix.id.clone(),
            ),
            (
                "AGENTBOX_LIBKRUN_NIX_DISK_LABEL".to_owned(),
                self.nix.label.clone(),
            ),
            (
                "AGENTBOX_LIBKRUN_CONTAINERS_STORAGE".to_owned(),
                "1".to_owned(),
            ),
            (
                "AGENTBOX_LIBKRUN_CONTAINERS_DISK_ID".to_owned(),
                self.containers.id.clone(),
            ),
            (
                "AGENTBOX_LIBKRUN_CONTAINERS_DISK_LABEL".to_owned(),
                self.containers.label.clone(),
            ),
        ]
    }
}

pub(crate) trait MicrovmPersistentDiskPreparer {
    fn prepare(&self, state_root: &Path) -> Result<MicrovmPersistentDisks>;
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct HostMicrovmPersistentDiskPreparer;

impl MicrovmPersistentDiskPreparer for HostMicrovmPersistentDiskPreparer {
    fn prepare(&self, state_root: &Path) -> Result<MicrovmPersistentDisks> {
        prepare_with_runner(state_root, &HostRawImageCommandRunner)
    }
}

fn prepare_with_runner(
    state_root: &Path,
    runner: &impl raw_btrfs::RawImageCommandRunner,
) -> Result<MicrovmPersistentDisks> {
    let nix = raw_btrfs::prepare_with_runner(state_root, &MICROVM_NIX_DISK_SPEC, runner)
        .context("failed to prepare microvm persistent dev cache disk")?;
    let containers =
        raw_btrfs::prepare_with_runner(state_root, &MICROVM_CONTAINERS_DISK_SPEC, runner)
            .context("failed to prepare microvm persistent dev cache disk")?;
    Ok(MicrovmPersistentDisks { nix, containers })
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;
    use crate::runtime::libkrun::raw_btrfs::RawBtrfsDiskStatus;
    use anyhow::Result;
    use std::cell::RefCell;
    use std::path::{Path, PathBuf};

    #[derive(Debug, Clone)]
    pub(crate) struct FakePersistentDiskPreparer {
        pub(crate) calls: std::rc::Rc<RefCell<Vec<PathBuf>>>,
        pub(crate) fail: bool,
    }

    impl FakePersistentDiskPreparer {
        pub(crate) fn ok(calls: std::rc::Rc<RefCell<Vec<PathBuf>>>) -> Self {
            Self { calls, fail: false }
        }

        pub(crate) fn failing(calls: std::rc::Rc<RefCell<Vec<PathBuf>>>) -> Self {
            Self { calls, fail: true }
        }
    }

    impl MicrovmPersistentDiskPreparer for FakePersistentDiskPreparer {
        fn prepare(&self, state_root: &Path) -> Result<MicrovmPersistentDisks> {
            self.calls.borrow_mut().push(state_root.to_path_buf());
            if self.fail {
                anyhow::bail!("fake disk prep failed");
            }
            Ok(MicrovmPersistentDisks {
                nix: RawBtrfsDisk {
                    path: state_root.join(MICROVM_NIX_DISK_FILE_NAME),
                    id: MICROVM_NIX_DISK_ID.to_owned(),
                    label: MICROVM_NIX_DISK_LABEL.to_owned(),
                    size_bytes: MICROVM_DISK_SIZE_BYTES,
                    status: RawBtrfsDiskStatus::Reused,
                },
                containers: RawBtrfsDisk {
                    path: state_root.join(MICROVM_CONTAINERS_DISK_FILE_NAME),
                    id: MICROVM_CONTAINERS_DISK_ID.to_owned(),
                    label: MICROVM_CONTAINERS_DISK_LABEL.to_owned(),
                    size_bytes: MICROVM_DISK_SIZE_BYTES,
                    status: RawBtrfsDiskStatus::Reused,
                },
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::libkrun::raw_btrfs::{RawBtrfsDiskStatus, test_support::FakeRunner};
    use std::fs::File;

    #[test]
    fn prepares_workspace_scoped_nix_and_container_store_raw_btrfs_disks() {
        let temp = tempfile::tempdir().expect("tempdir");
        let runner = FakeRunner::default();

        let disks = prepare_with_runner(temp.path(), &runner).expect("disks should prepare");

        assert_eq!(disks.nix.status, RawBtrfsDiskStatus::Created);
        assert_eq!(disks.nix.path, temp.path().join(MICROVM_NIX_DISK_FILE_NAME));
        assert_eq!(disks.nix.id, MICROVM_NIX_DISK_ID);
        assert_eq!(disks.nix.label, MICROVM_NIX_DISK_LABEL);
        assert_eq!(
            disks.containers.path,
            temp.path().join(MICROVM_CONTAINERS_DISK_FILE_NAME)
        );
        assert_eq!(disks.containers.id, MICROVM_CONTAINERS_DISK_ID);
        assert_eq!(disks.containers.label, MICROVM_CONTAINERS_DISK_LABEL);
        assert_eq!(runner.mkfs_call_count(), 2);
    }

    #[test]
    fn reuses_existing_valid_disks_without_formatting() {
        let temp = tempfile::tempdir().expect("tempdir");
        for name in [
            MICROVM_NIX_DISK_FILE_NAME,
            MICROVM_CONTAINERS_DISK_FILE_NAME,
        ] {
            let file = File::create(temp.path().join(name)).expect("disk file");
            file.set_len(MICROVM_DISK_SIZE_BYTES).expect("disk size");
        }
        let runner = FakeRunner::with_probe("btrfs");
        runner.push_probe("btrfs");

        let disks = prepare_with_runner(temp.path(), &runner).expect("disks should prepare");

        assert_eq!(disks.nix.status, RawBtrfsDiskStatus::Reused);
        assert_eq!(disks.containers.status, RawBtrfsDiskStatus::Reused);
        assert_eq!(runner.mkfs_call_count(), 0);
    }

    #[test]
    fn disk_validation_failure_is_classified_as_microvm_persistent_dev_cache() {
        let temp = tempfile::tempdir().expect("tempdir");
        let file = File::create(temp.path().join(MICROVM_NIX_DISK_FILE_NAME)).expect("disk file");
        file.set_len(MICROVM_DISK_SIZE_BYTES).expect("disk size");
        let runner = FakeRunner::default();
        runner.push_probe_error("blkid exploded");

        let err = prepare_with_runner(temp.path(), &runner).expect_err("disk prep should fail");

        assert!(format!("{err:#}").contains("failed to prepare microvm persistent dev cache disk"));
        assert!(format!("{err:#}").contains("blkid exploded"));
    }

    #[test]
    fn adds_compatibility_env_bridge_for_guest_reusable_components() {
        let temp = tempfile::tempdir().expect("tempdir");
        let runner = FakeRunner::default();
        let disks = prepare_with_runner(temp.path(), &runner).expect("disks should prepare");

        let env = disks.env_pairs();

        assert!(env.contains(&("AGENTBOX_LIBKRUN_NIX_OVERLAY".to_owned(), "1".to_owned())));
        assert!(env.contains(&(
            "AGENTBOX_LIBKRUN_NIX_DISK_ID".to_owned(),
            "agentbox-nix".to_owned()
        )));
        assert!(env.contains(&(
            "AGENTBOX_LIBKRUN_CONTAINERS_STORAGE".to_owned(),
            "1".to_owned()
        )));
        assert!(env.contains(&(
            "AGENTBOX_LIBKRUN_CONTAINERS_DISK_ID".to_owned(),
            "agentbox-containers".to_owned()
        )));
    }
}
