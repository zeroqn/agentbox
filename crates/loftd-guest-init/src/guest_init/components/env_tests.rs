use crate::guest_init::components::env::{
    CONTAINERS_STORE_ENV, ContainerStoreBackend, ENTER_AS_ROOT_ENV, LoftdEnv,
    RAW_CONTAINER_DISK_ID, RAW_CONTAINER_DISK_LABEL, RAW_NIX_DISK_ID, RAW_NIX_DISK_LABEL,
};
use std::sync::Mutex;

static ENV_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn internal_runtime_disk_contract_defaults_match_host_contract() {
    assert_eq!(RAW_NIX_DISK_ID, "loftd-nix");
    assert_eq!(RAW_NIX_DISK_LABEL, "LOFTD_NIX");
    assert_eq!(RAW_CONTAINER_DISK_ID, "loftd-containers");
    assert_eq!(RAW_CONTAINER_DISK_LABEL, "LOFTD_CONTAINERS");
    assert_eq!(ENTER_AS_ROOT_ENV, "LOFTD_ENTER_AS_ROOT");
}

#[test]
fn internal_runtime_parses_raw_disk_container_store_backend_contract() {
    let _guard = ENV_LOCK.lock().expect("env test lock");
    // SAFETY: test mutates process env in a small single-threaded assertion.
    unsafe {
        std::env::set_var("LOFTD_CONTAINERS_STORAGE", "1");
        std::env::set_var(CONTAINERS_STORE_ENV, "raw-disk");
    }
    let raw = LoftdEnv::from_process_env().expect("raw backend should parse");
    unsafe {
        std::env::remove_var("LOFTD_CONTAINERS_STORAGE");
        std::env::remove_var(CONTAINERS_STORE_ENV);
    }

    assert!(raw.containers_storage);
    assert_eq!(raw.container_store_backend, ContainerStoreBackend::RawDisk);
}

#[test]
fn internal_runtime_legacy_container_storage_defaults_to_raw_disk() {
    let _guard = ENV_LOCK.lock().expect("env test lock");
    // SAFETY: test mutates process env in a small single-threaded assertion.
    unsafe {
        std::env::set_var("LOFTD_CONTAINERS_STORAGE", "1");
        std::env::remove_var(CONTAINERS_STORE_ENV);
    }
    let parsed = LoftdEnv::from_process_env().expect("legacy env should parse");
    unsafe {
        std::env::remove_var("LOFTD_CONTAINERS_STORAGE");
    }

    assert_eq!(
        parsed.container_store_backend,
        ContainerStoreBackend::RawDisk
    );
}

#[test]
fn internal_runtime_rejects_unknown_container_store_backend() {
    let _guard = ENV_LOCK.lock().expect("env test lock");
    // SAFETY: test mutates process env in a small single-threaded assertion.
    unsafe {
        std::env::set_var(CONTAINERS_STORE_ENV, "overlay");
    }
    let err = LoftdEnv::from_process_env().expect_err("unknown backend should fail");
    unsafe {
        std::env::remove_var(CONTAINERS_STORE_ENV);
    }

    assert!(err.to_string().contains(CONTAINERS_STORE_ENV));
}

#[test]
fn internal_runtime_rejects_bind_container_store_backend() {
    let _guard = ENV_LOCK.lock().expect("env test lock");
    // SAFETY: test mutates process env in a small single-threaded assertion.
    unsafe {
        std::env::set_var(CONTAINERS_STORE_ENV, "bind");
    }
    let err = LoftdEnv::from_process_env().expect_err("bind backend should fail");
    unsafe {
        std::env::remove_var(CONTAINERS_STORE_ENV);
    }

    assert!(err.to_string().contains(CONTAINERS_STORE_ENV));
}

#[test]
fn internal_runtime_parses_authoritative_host_nix_overlay_marker() {
    let _guard = ENV_LOCK.lock().expect("env test lock");
    // SAFETY: test mutates process env in a small single-threaded assertion.
    unsafe {
        std::env::set_var("LOFTD_NIX_OVERLAY", "1");
        std::env::set_var("LOFTD_NIX_HOST_OVERLAY", "1");
    }
    let parsed = LoftdEnv::from_process_env().expect("env should parse");
    unsafe {
        std::env::remove_var("LOFTD_NIX_OVERLAY");
        std::env::remove_var("LOFTD_NIX_HOST_OVERLAY");
    }

    assert!(parsed.nix_overlay);
    assert!(parsed.nix_host_overlay);
}
