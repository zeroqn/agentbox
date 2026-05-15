use std::env;
use std::fs;
use std::os::unix::fs::PermissionsExt;

use tempfile::tempdir;

use crate::guest_init::components::rootless::idmap::{
    HELPER_DIR, helper_metadata_is_ready, installed_helper_is_ready, source_helper_on_path,
};

#[test]
fn idmap_source_lookup_skips_installed_helper_dir_when_alternate_exists() {
    let temp = tempdir().unwrap();
    let helper_dir = temp.path().join("idmap-bin");
    let source_dir = temp.path().join("source-bin");
    fs::create_dir_all(&helper_dir).unwrap();
    fs::create_dir_all(&source_dir).unwrap();
    make_executable(&helper_dir.join("newuidmap"));
    make_executable(&source_dir.join("newuidmap"));
    let old_path = env::var_os("PATH");
    unsafe {
        env::set_var(
            "PATH",
            format!("{}:{}", helper_dir.display(), source_dir.display()),
        )
    };

    let source = source_helper_on_path("newuidmap", &helper_dir).unwrap();

    restore_path(old_path);
    assert_eq!(source, source_dir.join("newuidmap"));
}

#[test]
fn idmap_source_lookup_allows_existing_installed_helper_as_idempotent_fallback_when_root_owned() {
    if unsafe { libc::geteuid() } != 0 {
        return;
    }
    let temp = tempdir().unwrap();
    let helper_dir = temp.path().join("idmap-bin");
    fs::create_dir_all(&helper_dir).unwrap();
    make_setuid_executable(&helper_dir.join("newuidmap"));
    let old_path = env::var_os("PATH");
    unsafe { env::set_var("PATH", helper_dir.display().to_string()) };

    let source = source_helper_on_path("newuidmap", &helper_dir).unwrap();

    restore_path(old_path);
    assert_eq!(source, helper_dir.join("newuidmap"));
}

#[test]
fn idmap_source_lookup_rejects_non_setuid_installed_helper_without_alternate() {
    let temp = tempdir().unwrap();
    let helper_dir = temp.path().join("idmap-bin");
    fs::create_dir_all(&helper_dir).unwrap();
    make_executable(&helper_dir.join("newuidmap"));
    let old_path = env::var_os("PATH");
    unsafe { env::set_var("PATH", helper_dir.display().to_string()) };

    let err = source_helper_on_path("newuidmap", &helper_dir).unwrap_err();

    restore_path(old_path);
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
    make_setuid_executable(&helper);
    if unsafe { libc::geteuid() } != 0 {
        assert!(!installed_helper_is_ready(&helper));
    }
}

#[test]
fn idmap_helper_dir_stays_agentbox_run_path() {
    assert_eq!(HELPER_DIR, "/run/agentbox/idmap-bin");
}

fn make_executable(path: &std::path::Path) {
    write_helper_with_mode(path, 0o755);
}

fn make_setuid_executable(path: &std::path::Path) {
    write_helper_with_mode(path, 0o4755);
}

fn write_helper_with_mode(path: &std::path::Path, mode: u32) {
    fs::write(path, "#!/bin/sh\n").unwrap();
    let mut perms = fs::metadata(path).unwrap().permissions();
    perms.set_mode(mode);
    fs::set_permissions(path, perms).unwrap();
}

fn restore_path(old_path: Option<std::ffi::OsString>) {
    match old_path {
        Some(value) => unsafe { env::set_var("PATH", value) },
        None => unsafe { env::remove_var("PATH") },
    }
}
