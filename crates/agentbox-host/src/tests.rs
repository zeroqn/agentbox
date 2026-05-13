use super::*;

#[test]
fn task_hostname_uses_current_directory_name() {
    assert_eq!(
        derive_task_hostname(std::path::Path::new("/tmp/project")),
        "project-agentbox"
    );
}

#[test]
fn task_hostname_sanitizes_current_directory_name() {
    assert_eq!(
        derive_task_hostname(std::path::Path::new("/tmp/My repo.name!")),
        "my-repo-name-agentbox"
    );
}

#[test]
fn task_hostname_falls_back_when_directory_name_has_no_slug_chars() {
    assert_eq!(
        derive_task_hostname(std::path::Path::new("/tmp/!!!")),
        "workspace-agentbox"
    );
}

#[test]
fn task_container_name_prefixes_unique_suffix_with_current_directory_name() {
    assert_eq!(
        derive_task_container_name_with_suffix(
            std::path::Path::new("/tmp/My repo.name!"),
            "random-suffix"
        ),
        "my-repo-name-random-suffix"
    );
}

#[test]
fn task_container_name_falls_back_when_directory_name_has_no_slug_chars() {
    assert_eq!(
        derive_task_container_name_with_suffix(std::path::Path::new("/tmp/!!!"), "random-suffix"),
        "workspace-random-suffix"
    );
}

const ENTRYPOINT: &str = include_str!("../../../nix/image/entrypoint.nix");
const LAYERS: &str = include_str!("../../../nix/image/layers.nix");
const CONTAINER_NIX: &str = include_str!("../../../nix/image/container.nix");
const AGENTBOX_RUST_NIX: &str = include_str!("../../../nix/pkgs/agentbox-rust.nix");

#[test]
fn image_entrypoint_remains_agentbox_entrypoint_for_normal_container_mode() {
    assert!(CONTAINER_NIX.contains(r#"Entrypoint = [ "${entrypoint}/bin/agentbox-entrypoint" ];"#));
    let config_start = CONTAINER_NIX
        .find("config = {")
        .expect("image config should exist");
    assert!(!CONTAINER_NIX[config_start..].contains("agentbox-guest-init"));
}

#[test]
fn entrypoint_dispatches_libkrun_to_rust_guest_init_early() {
    let dispatch_gate = r#"if [ "''${AGENTBOX_GUEST_INIT_DISABLE:-}" != "1" ] \
    && { [ "''${AGENTBOX_LIBKRUN_NIX_OVERLAY:-}" = "1" ] || [ "''${AGENTBOX_LIBKRUN_CONTAINERS_STORAGE:-}" = "1" ]; }; then"#;
    assert!(ENTRYPOINT.contains(dispatch_gate));
    assert!(ENTRYPOINT.contains(
        r#"export AGENTBOX_FISH_CONFIG_SOURCE=${fishConfig}/share/agentbox/fish/conf.d/agentbox-starship.fish"#
    ));
    assert!(ENTRYPOINT.contains(
        r#"export AGENTBOX_STARSHIP_CONFIG_SOURCE=${starshipConfig}/share/agentbox/starship.toml"#
    ));
    assert!(ENTRYPOINT
        .contains(r#"exec ${agentboxMuslPackage}/bin/agentbox-guest-init libkrun enter -- "$@""#));

    let fish_config_export = ENTRYPOINT
        .find("AGENTBOX_FISH_CONFIG_SOURCE")
        .expect("fish config source should be exported");
    let starship_config_export = ENTRYPOINT
        .find("AGENTBOX_STARSHIP_CONFIG_SOURCE")
        .expect("starship config source should be exported");
    let dispatch = ENTRYPOINT
        .find("agentbox-guest-init libkrun enter")
        .expect("libkrun dispatch should exist");
    let bash_env_setup = ENTRYPOINT
        .find("export USER=dev")
        .expect("normal Bash body should remain");
    let legacy_podman_bootstrap = ENTRYPOINT
        .find("bootstrap_libkrun_containers_storage()")
        .expect("fallback legacy libkrun function should remain behind disable guard");
    assert!(fish_config_export < dispatch);
    assert!(starship_config_export < dispatch);
    assert!(dispatch < bash_env_setup);
    assert!(dispatch < legacy_podman_bootstrap);
}

#[test]
fn nix_agentbox_binary_knows_libkrun_guest_init_image_path() {
    assert!(AGENTBOX_RUST_NIX.contains(
        r#"AGENTBOX_LIBKRUN_GUEST_INIT_TARGET = "${agentboxMuslPackage}/bin/agentbox-guest-init";"#
    ));
}

#[test]
fn entrypoint_preserves_normal_container_bash_path() {
    assert!(ENTRYPOINT.contains("export USER=dev"));
    assert!(ENTRYPOINT.contains("materialize_writable_dir()"));
    assert!(ENTRYPOINT.contains("exec \"$@\""));
    assert!(!ENTRYPOINT.contains("agentbox-guest-init container enter"));
}

#[test]
fn podman_wrapper_waits_only_for_libkrun_container_storage() {
    assert!(LAYERS.contains(r#"if [ "''${AGENTBOX_LIBKRUN_CONTAINERS_STORAGE:-}" = "1" ]; then"#));
    assert!(LAYERS.contains("agentbox-guest-init libkrun podman wait"));
    let gate = LAYERS
        .find("AGENTBOX_LIBKRUN_CONTAINERS_STORAGE")
        .expect("libkrun container storage gate should exist");
    let wait = LAYERS
        .find("agentbox-guest-init libkrun podman wait")
        .expect("podman wait should exist");
    let exec = LAYERS
        .find(r#"exec ${podman}/bin/podman "$@""#)
        .expect("real podman exec should exist");
    assert!(gate < wait);
    assert!(wait < exec);
}

#[test]
fn podman_wrapper_unsets_compat_env_before_execing_real_podman() {
    for required in [
        "podmanCommandCompat",
        "pkgs.writeShellScriptBin \"podman\"",
        "unset LD_PRELOAD",
        "unset NSS_WRAPPER_PASSWD",
        "unset NSS_WRAPPER_GROUP",
        r#"exec ${podman}/bin/podman "$@""#,
    ] {
        assert!(LAYERS.contains(required), "missing {required}");
    }
}

#[test]
fn image_includes_btrfs_progs_for_guest_bootstrap() {
    assert!(LAYERS.contains("pkgs.btrfs-progs"));
}

#[test]
fn image_includes_rootless_podman_stack_without_fuse_overlayfs() {
    for required in [
        "rootlessPodmanImagePackages",
        "podmanCommandCompat",
        "podman",
        "crun",
        "pkgs.conmon",
        "pkgs.netavark",
        "pkgs.aardvark-dns",
        "pkgs.passt",
        "pkgs.shadow",
    ] {
        assert!(LAYERS.contains(required), "missing {required}");
    }
    assert!(!LAYERS.contains("fuse-overlayfs"));
}
