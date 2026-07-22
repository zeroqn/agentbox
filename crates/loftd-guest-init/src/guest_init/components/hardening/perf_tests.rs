use std::fs;

use super::configure_at;

#[test]
fn configure_at_leaves_perf_restricted_by_default() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let path = temp.path().join("perf_event_paranoid");
    fs::write(&path, "3\n").expect("perf sysctl file should be created");

    configure_at(&path, false).expect("perf restriction should remain unchanged");

    assert_eq!(fs::read_to_string(path).unwrap(), "3\n");
}

#[test]
fn configure_at_relaxes_perf_when_requested() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let path = temp.path().join("perf_event_paranoid");
    fs::write(&path, "3\n").expect("perf sysctl file should be created");

    configure_at(&path, true).expect("perf should be relaxed");

    assert_eq!(fs::read_to_string(path).unwrap(), "-1\n");
}

#[test]
fn configure_at_reports_context_for_missing_target() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let path = temp
        .path()
        .join("missing-parent")
        .join("perf_event_paranoid");

    let err = configure_at(&path, true).expect_err("missing perf target should fail");
    let message = format!("{err:#}");

    assert!(message.contains("failed to open"), "{message}");
    assert!(message.contains("perf_event_paranoid=-1"), "{message}");
    assert!(message.contains(path.to_str().unwrap()), "{message}");
}
