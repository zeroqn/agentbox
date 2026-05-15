use std::fs;

use super::restrict_at;

#[test]
fn restrict_at_writes_exact_dmesg_restriction() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let path = temp.path().join("dmesg_restrict");
    fs::write(&path, "").expect("test sysctl file should be created");

    restrict_at(&path).expect("dmesg restriction should be written");

    assert_eq!(fs::read_to_string(path).unwrap(), "1\n");
}

#[test]
fn restrict_at_overwrites_existing_dmesg_restriction() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let path = temp.path().join("dmesg_restrict");
    fs::write(&path, "0\nextra").expect("test sysctl file should be created");

    restrict_at(&path).expect("dmesg restriction should be overwritten");

    assert_eq!(fs::read_to_string(path).unwrap(), "1\n");
}

#[test]
fn restrict_at_reports_context_for_missing_target() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let path = temp.path().join("missing-parent").join("dmesg_restrict");

    let err = restrict_at(&path).expect_err("missing target should fail");
    let message = format!("{err:#}");

    assert!(message.contains("failed to open"), "{message}");
    assert!(message.contains("dmesg restriction"), "{message}");
    assert!(message.contains(path.to_str().unwrap()), "{message}");
}

#[test]
fn restrict_at_reports_context_for_unwritable_target() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let path = temp.path().join("dmesg_restrict");
    fs::create_dir(&path).expect("directory target should be created");

    let err = restrict_at(&path).expect_err("directory target should fail");
    let message = format!("{err:#}");

    assert!(message.contains("failed to open"), "{message}");
    assert!(message.contains("dmesg restriction"), "{message}");
    assert!(message.contains(path.to_str().unwrap()), "{message}");
}
