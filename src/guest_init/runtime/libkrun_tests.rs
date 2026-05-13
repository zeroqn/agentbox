use crate::guest_init::root::home::DevIdentity;
use crate::guest_init::runtime::libkrun::{
    derive_shell_environment, normalize_resolv_conf, RAW_CONTAINER_DISK_ID,
    RAW_CONTAINER_DISK_LABEL, RAW_NIX_DISK_ID, RAW_NIX_DISK_LABEL,
};
use std::path::PathBuf;

#[test]
fn libkrun_runtime_derives_parent_shell_environment_before_podman_prep() {
    let identity = DevIdentity::new(1234, 1235, PathBuf::from("/nix/store/fish/bin/fish"));
    let shell_env = derive_shell_environment(&identity, true);
    assert!(shell_env
        .vars
        .contains(&("HOME".to_owned(), "/home/dev".to_owned())));
    assert!(shell_env
        .vars
        .contains(&("USER".to_owned(), "dev".to_owned())));
    assert!(shell_env
        .vars
        .contains(&("XDG_RUNTIME_DIR".to_owned(), "/run/user/1234".to_owned())));
    assert!(shell_env
        .vars
        .iter()
        .any(|(key, value)| key == "PATH" && value.starts_with("/run/agentbox/idmap-bin:")));
    assert_eq!(shell_env.tmpdir, PathBuf::from("/home/dev/.cache/tmp"));
}

#[test]
fn libkrun_runtime_disk_contract_defaults_match_host_contract() {
    assert_eq!(RAW_NIX_DISK_ID, "agentbox-nix");
    assert_eq!(RAW_NIX_DISK_LABEL, "AGENTBOX_NIX");
    assert_eq!(RAW_CONTAINER_DISK_ID, "agentbox-containers");
    assert_eq!(RAW_CONTAINER_DISK_LABEL, "AGENTBOX_CONTAINERS");
}

#[test]
fn libkrun_passt_dns_normalizes_resolv_conf() {
    assert_eq!(
        normalize_resolv_conf(Some(
            "search example.test\nnameserver 8.8.8.8\noptions ndots:1\n"
        )),
        "nameserver 169.254.1.1\nsearch example.test\nnameserver 8.8.8.8\noptions ndots:1\n"
    );
    assert_eq!(
        normalize_resolv_conf(Some("nameserver 8.8.8.8\nnameserver 169.254.1.1\n")),
        "nameserver 169.254.1.1\nnameserver 8.8.8.8\n"
    );
    assert_eq!(normalize_resolv_conf(None), "nameserver 169.254.1.1\n");
}
