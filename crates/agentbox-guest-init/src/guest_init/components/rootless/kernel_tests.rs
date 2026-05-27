use std::fs;
use std::os::unix::fs::PermissionsExt;

use tempfile::tempdir;

use super::prepare_kvm_device_at;

#[test]
fn kvm_device_prep_is_noop_when_device_is_absent() {
    let temp = tempdir().unwrap();
    let missing_kvm = temp.path().join("kvm");

    prepare_kvm_device_at(&missing_kvm).unwrap();

    assert!(!missing_kvm.exists());
}

#[test]
fn kvm_device_prep_makes_existing_device_world_accessible() {
    let temp = tempdir().unwrap();
    let kvm = temp.path().join("kvm");
    fs::write(&kvm, "").unwrap();
    let mut permissions = fs::metadata(&kvm).unwrap().permissions();
    permissions.set_mode(0o600);
    fs::set_permissions(&kvm, permissions).unwrap();

    prepare_kvm_device_at(&kvm).unwrap();

    let mode = fs::metadata(&kvm).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o666);
}
