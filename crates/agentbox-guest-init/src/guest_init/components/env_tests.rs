use crate::guest_init::components::env::{
    RAW_CONTAINER_DISK_ID, RAW_CONTAINER_DISK_LABEL, RAW_NIX_DISK_ID, RAW_NIX_DISK_LABEL,
};

#[test]
fn libkrun_runtime_disk_contract_defaults_match_host_contract() {
    assert_eq!(RAW_NIX_DISK_ID, "agentbox-nix");
    assert_eq!(RAW_NIX_DISK_LABEL, "AGENTBOX_NIX");
    assert_eq!(RAW_CONTAINER_DISK_ID, "agentbox-containers");
    assert_eq!(RAW_CONTAINER_DISK_LABEL, "AGENTBOX_CONTAINERS");
}
