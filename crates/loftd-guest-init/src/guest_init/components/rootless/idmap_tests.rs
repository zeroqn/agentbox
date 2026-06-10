use std::env;
use std::fs;
use std::os::unix::fs::PermissionsExt;

use tempfile::tempdir;

use crate::guest_init::components::rootless::idmap::{
    WRAPPER_BIN_DIR, helper_metadata_is_ready, installed_helper_is_ready, source_helper_in_path,
};

#[test]
fn idmap_source_lookup_skips_installed_helper_dir_when_alternate_exists() {
    let temp = tempdir().unwrap();
    let wrapper_dir = temp.path().join("wrappers/bin");
    let source_dir = temp.path().join("source-bin");
    fs::create_dir_all(&wrapper_dir).unwrap();
    fs::create_dir_all(&source_dir).unwrap();
    make_executable(&wrapper_dir.join("newuidmap"));
    make_executable(&source_dir.join("newuidmap"));
    let path = env::join_paths([wrapper_dir.as_path(), source_dir.as_path()]).unwrap();

    let source = source_helper_in_path("newuidmap", &wrapper_dir, &path).unwrap();

    assert_eq!(source, source_dir.join("newuidmap"));
}

#[test]
fn idmap_source_lookup_allows_existing_installed_helper_as_idempotent_fallback_when_root_owned() {
    if unsafe { libc::geteuid() } != 0 {
        return;
    }
    let temp = tempdir().unwrap();
    let wrapper_dir = temp.path().join("wrappers/bin");
    fs::create_dir_all(&wrapper_dir).unwrap();
    make_setuid_executable(&wrapper_dir.join("newuidmap"));
    let path = env::join_paths([wrapper_dir.as_path()]).unwrap();

    let source = source_helper_in_path("newuidmap", &wrapper_dir, &path).unwrap();

    assert_eq!(source, wrapper_dir.join("newuidmap"));
}

#[test]
fn idmap_source_lookup_rejects_non_setuid_installed_helper_without_alternate() {
    let temp = tempdir().unwrap();
    let wrapper_dir = temp.path().join("wrappers/bin");
    fs::create_dir_all(&wrapper_dir).unwrap();
    make_executable(&wrapper_dir.join("newuidmap"));
    let path = env::join_paths([wrapper_dir.as_path()]).unwrap();

    let err = source_helper_in_path("newuidmap", &wrapper_dir, &path).unwrap_err();

    assert!(err.to_string().contains("required tool 'newuidmap'"));
}

#[test]
fn idmap_helper_metadata_readiness_requires_root_owned_setuid_executable_file() {
    assert!(!helper_metadata_is_ready(false, 0o4755, 0, 0));
    assert!(!helper_metadata_is_ready(true, 0o0755, 0, 0));
    assert!(!helper_metadata_is_ready(true, 0o4755, 1000, 0));
    assert!(!helper_metadata_is_ready(true, 0o4755, 0, 1000));
    assert!(helper_metadata_is_ready(true, 0o4755, 0, 0));
}

#[test]
fn idmap_installed_helper_readiness_rejects_non_root_owned_test_file() {
    let temp = tempdir().unwrap();
    let helper = temp.path().join("newuidmap");

    assert!(!installed_helper_is_ready(&helper));
    if unsafe { libc::geteuid() } == 0 {
        // This test relies on the temp file being owned by the non-root test
        // user; root-owned readiness is covered by the PATH fallback test.
        return;
    }
    match try_make_setuid_executable(&helper) {
        Ok(()) => assert!(!installed_helper_is_ready(&helper)),
        Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied => {
            // Some CI sandboxes deny setting the setuid bit on temp files.
        }
        Err(err) => panic!("failed to create setuid helper fixture: {err}"),
    }
}

#[test]
fn idmap_wrapper_dir_stays_loftd_run_path() {
    assert_eq!(WRAPPER_BIN_DIR, "/run/loftd/wrappers/bin");
}

fn make_executable(path: &std::path::Path) {
    write_helper_with_mode(path, 0o755).unwrap();
}

fn make_setuid_executable(path: &std::path::Path) {
    try_make_setuid_executable(path).unwrap();
}

fn try_make_setuid_executable(path: &std::path::Path) -> std::io::Result<()> {
    write_helper_with_mode(path, 0o4755)
}

fn write_helper_with_mode(path: &std::path::Path, mode: u32) -> std::io::Result<()> {
    fs::write(path, "#!/bin/sh\n")?;
    let mut perms = fs::metadata(path)?.permissions();
    perms.set_mode(mode);
    fs::set_permissions(path, perms)
}
