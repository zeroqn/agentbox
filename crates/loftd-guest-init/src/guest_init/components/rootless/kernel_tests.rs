use std::fs;
use std::os::unix::fs::PermissionsExt;

use tempfile::tempdir;

use super::{linux_major, linux_minor, prepare_kvm_device_at, prepare_tun_device_at};

fn noop_tun_probe(path: &std::path::Path) -> anyhow::Result<()> {
    if path.exists() {
        Ok(())
    } else {
        anyhow::bail!("missing probe target")
    }
}

#[test]
fn tun_device_prep_fails_when_device_is_absent() {
    let temp = tempdir().unwrap();
    let missing_tun = temp.path().join("tun");

    let err = prepare_tun_device_at(&missing_tun, noop_tun_probe)
        .expect_err("missing tun should fail loud");

    assert!(format!("{err:#}").contains("TUN device is missing"));
}

#[test]
fn tun_device_prep_rejects_non_character_device() {
    let temp = tempdir().unwrap();
    let tun = temp.path().join("tun");
    fs::write(&tun, "").unwrap();

    let err =
        prepare_tun_device_at(&tun, noop_tun_probe).expect_err("regular file should be rejected");

    assert!(format!("{err:#}").contains("not a character device"));
}

#[test]
fn linux_device_number_helpers_match_tun_major_minor() {
    let dev = libc::makedev(10, 200);

    assert_eq!(linux_major(dev), 10);
    assert_eq!(linux_minor(dev), 200);
}

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
