use crate::guest_init::components::env::{
    ENTER_AS_ROOT_ENV, RAW_CONTAINER_DISK_ID, RAW_CONTAINER_DISK_LABEL, RAW_NIX_DISK_ID,
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
