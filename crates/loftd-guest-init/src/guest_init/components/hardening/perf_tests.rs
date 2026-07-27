use std::fs;

use super::configure_at;

#[test]
fn configure_at_leaves_perf_restricted_by_default() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let perf_path = temp.path().join("perf_event_paranoid");
    let kptr_path = temp.path().join("kptr_restrict");
    fs::write(&perf_path, "3\n").expect("perf sysctl file should be created");
    fs::write(&kptr_path, "1\n").expect("kptr sysctl file should be created");

    configure_at(&perf_path, &kptr_path, false).expect("perf restrictions should remain unchanged");

    assert_eq!(fs::read_to_string(perf_path).unwrap(), "3\n");
    assert_eq!(fs::read_to_string(kptr_path).unwrap(), "1\n");
}

#[test]
fn configure_at_relaxes_perf_when_requested() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let perf_path = temp.path().join("perf_event_paranoid");
    let kptr_path = temp.path().join("kptr_restrict");
    fs::write(&perf_path, "3\n").expect("perf sysctl file should be created");
    fs::write(&kptr_path, "1\n").expect("kptr sysctl file should be created");

    configure_at(&perf_path, &kptr_path, true).expect("perf should be relaxed");

    assert_eq!(fs::read_to_string(perf_path).unwrap(), "-1\n");
    assert_eq!(fs::read_to_string(kptr_path).unwrap(), "0\n");
}

#[test]
fn configure_at_reports_context_for_missing_perf_target() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let perf_path = temp
        .path()
        .join("missing-parent")
        .join("perf_event_paranoid");
    let kptr_path = temp.path().join("kptr_restrict");
    fs::write(&kptr_path, "1\n").expect("kptr sysctl file should be created");

    let err =
        configure_at(&perf_path, &kptr_path, true).expect_err("missing perf target should fail");
    let message = format!("{err:#}");

    assert!(message.contains("failed to open"), "{message}");
    assert!(message.contains("perf_event_paranoid=-1"), "{message}");
    assert!(message.contains(perf_path.to_str().unwrap()), "{message}");
}

#[test]
fn configure_at_reports_context_for_missing_kptr_target() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let perf_path = temp.path().join("perf_event_paranoid");
    let kptr_path = temp.path().join("missing-parent").join("kptr_restrict");
    fs::write(&perf_path, "3\n").expect("perf sysctl file should be created");

    let err =
        configure_at(&perf_path, &kptr_path, true).expect_err("missing kptr target should fail");
    let message = format!("{err:#}");

    assert!(message.contains("failed to open"), "{message}");
    assert!(message.contains("kptr_restrict=0"), "{message}");
    assert!(message.contains(kptr_path.to_str().unwrap()), "{message}");
}
