use crate::guest_init::components::env::{
    ENTER_AS_ROOT_ENV, LoftdEnv, RAW_CONTAINER_DISK_ID, RAW_CONTAINER_DISK_LABEL, RAW_NIX_DISK_ID,
    RAW_NIX_DISK_LABEL,
};

#[test]
fn internal_runtime_disk_contract_defaults_match_host_contract() {
    assert_eq!(RAW_NIX_DISK_ID, "loftd-nix");
    assert_eq!(RAW_NIX_DISK_LABEL, "LOFTD_NIX");
    assert_eq!(RAW_CONTAINER_DISK_ID, "loftd-containers");
    assert_eq!(RAW_CONTAINER_DISK_LABEL, "LOFTD_CONTAINERS");
    assert_eq!(ENTER_AS_ROOT_ENV, "LOFTD_ENTER_AS_ROOT");
}

#[test]
fn internal_runtime_parses_authoritative_host_nix_overlay_marker() {
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
