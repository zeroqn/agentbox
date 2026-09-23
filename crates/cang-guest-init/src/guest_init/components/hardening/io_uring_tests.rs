use std::fs;

use super::configure_at;

#[test]
fn configure_at_disables_io_uring_by_default() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let disabled_path = temp.path().join("io_uring_disabled");
    let group_path = temp.path().join("io_uring_group");
    fs::write(&disabled_path, "1\n").expect("disabled sysctl file should be created");
    fs::write(&group_path, "-1\n").expect("group sysctl file should be created");

    configure_at(&disabled_path, &group_path, false, 1000).expect("io_uring should be disabled");

    assert_eq!(fs::read_to_string(disabled_path).unwrap(), "2\n");
    assert_eq!(fs::read_to_string(group_path).unwrap(), "-1\n");
}

#[test]
fn configure_at_allows_the_dev_group_when_requested() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let disabled_path = temp.path().join("missing-io_uring_disabled");
    let group_path = temp.path().join("io_uring_group");
    fs::write(&group_path, "-1\n").expect("group sysctl file should be created");

    configure_at(&disabled_path, &group_path, true, 1000)
        .expect("dev group should be allowed to use io_uring");

    assert!(!disabled_path.exists());
    assert_eq!(fs::read_to_string(group_path).unwrap(), "1000\n");
}

#[test]
fn configure_at_reports_context_for_missing_disabled_target() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let disabled_path = temp.path().join("missing-parent").join("io_uring_disabled");
    let group_path = temp.path().join("io_uring_group");
    fs::write(&group_path, "-1\n").expect("group sysctl file should be created");

    let err = configure_at(&disabled_path, &group_path, false, 1000)
        .expect_err("missing disabled target should fail");
    let message = format!("{err:#}");

    assert!(message.contains("failed to open"), "{message}");
    assert!(message.contains("io_uring_disabled=2"), "{message}");
    assert!(
        message.contains(disabled_path.to_str().unwrap()),
        "{message}"
    );
}

#[test]
fn configure_at_reports_context_for_missing_group_target() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let disabled_path = temp.path().join("io_uring_disabled");
    let group_path = temp.path().join("missing-parent").join("io_uring_group");
    fs::write(&disabled_path, "1\n").expect("disabled sysctl file should be created");

    let err = configure_at(&disabled_path, &group_path, true, 1000)
        .expect_err("missing group target should fail");
    let message = format!("{err:#}");

    assert!(message.contains("failed to open"), "{message}");
    assert!(message.contains("io_uring_group=1000"), "{message}");
    assert!(message.contains(group_path.to_str().unwrap()), "{message}");
    assert_eq!(fs::read_to_string(disabled_path).unwrap(), "1\n");
}
