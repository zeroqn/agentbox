use std::env;
use std::fs;
use std::os::unix::fs::PermissionsExt;

use tempfile::tempdir;

use crate::guest_init::components::rootless::idmap::{source_helper_on_path, HELPER_DIR};

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
fn idmap_source_lookup_allows_existing_installed_helper_as_idempotent_fallback() {
    let temp = tempdir().unwrap();
    let helper_dir = temp.path().join("idmap-bin");
    fs::create_dir_all(&helper_dir).unwrap();
    make_executable(&helper_dir.join("newuidmap"));
    let old_path = env::var_os("PATH");
    unsafe { env::set_var("PATH", helper_dir.display().to_string()) };

    let source = source_helper_on_path("newuidmap", &helper_dir).unwrap();

    restore_path(old_path);
    assert_eq!(source, helper_dir.join("newuidmap"));
}

#[test]
fn idmap_helper_dir_stays_agentbox_run_path() {
    assert_eq!(HELPER_DIR, "/run/agentbox/idmap-bin");
}

fn make_executable(path: &std::path::Path) {
    fs::write(path, "#!/bin/sh\n").unwrap();
    let mut perms = fs::metadata(path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms).unwrap();
}

fn restore_path(old_path: Option<std::ffi::OsString>) {
    match old_path {
        Some(value) => unsafe { env::set_var("PATH", value) },
        None => unsafe { env::remove_var("PATH") },
    }
}
