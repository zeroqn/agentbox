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
const GUEST_CONTAINER_RUNTIME: &str =
    include_str!("../../agentbox-guest-init/src/guest_init/runtime/container.rs");

#[test]
fn image_entrypoint_invokes_guest_init_container_enter_directly() {
    assert!(CONTAINER_NIX.contains(
        r#"Entrypoint = [ "${agentboxMuslPackage}/bin/agentbox-guest-init" "container" "enter" "--" ];"#
    ));
    let config_start = CONTAINER_NIX
        .find("config = {")
        .expect("image config should exist");
    assert!(!CONTAINER_NIX[config_start..].contains("agentbox-entrypoint"));
}

#[test]
fn image_env_exposes_guest_init_runtime_payloads() {
    for required in [
        r#"SHELL=${pkgs.fish}/bin/fish"#,
        r#"AGENTBOX_FISH_CONFIG_SOURCE=${configPayloads.fishConfig}/share/agentbox/fish/conf.d/agentbox-starship.fish"#,
        r#"AGENTBOX_STARSHIP_CONFIG_SOURCE=${configPayloads.starshipConfig}/share/agentbox/starship.toml"#,
        r#"AGENTBOX_NSS_WRAPPER_LIB=${pkgs.nss_wrapper}/lib/libnss_wrapper.so"#,
    ] {
        assert!(CONTAINER_NIX.contains(required), "missing {required}");
    }
}

#[test]
fn container_guest_init_dispatches_libkrun_before_normal_setup() {
    for required in [
        "const LIBKRUN_NIX_OVERLAY_ENV",
        "const LIBKRUN_CONTAINERS_STORAGE_ENV",
        "fn should_dispatch_libkrun_from_env()",
        r#""libkrun".to_owned()"#,
        r#""enter".to_owned()"#,
        r#""--".to_owned()"#,
    ] {
        assert!(
            GUEST_CONTAINER_RUNTIME.contains(required),
            "missing {required}"
        );
    }

    let dispatch = GUEST_CONTAINER_RUNTIME
        .find("should_dispatch_libkrun_from_env()")
        .expect("libkrun dispatch gate should exist");
    let identity = GUEST_CONTAINER_RUNTIME
        .find("derive_identity_plan")
        .expect("normal identity setup should exist");
    assert!(dispatch < identity);
}

#[test]
fn nix_agentbox_binary_does_not_embed_libkrun_guest_init_image_path() {
    let removed_env = ["AGENTBOX", "LIBKRUN", "GUEST_INIT", "TARGET"].join("_");

    assert!(!AGENTBOX_RUST_NIX.contains(&removed_env));
}

#[test]
fn legacy_bash_entrypoint_is_not_the_image_config_entrypoint() {
    assert!(ENTRYPOINT.contains("export USER=dev"));
    assert!(ENTRYPOINT.contains("materialize_writable_dir()"));
    assert!(ENTRYPOINT.contains("exec \"$@\""));
    assert!(!CONTAINER_NIX.contains("${entrypoint}/bin/agentbox-entrypoint"));
}

#[test]
fn image_layers_include_guest_init_config_payloads() {
    assert!(LAYERS.contains("fishConfig"));
    assert!(LAYERS.contains("starshipConfig"));
    assert!(LAYERS.contains("agentboxMuslPackage"));
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
fn nix_wrapper_waits_and_probes_only_for_libkrun_nix_overlay() {
    assert!(LAYERS.contains(r#"if [ "''${AGENTBOX_LIBKRUN_NIX_OVERLAY:-}" = "1" ]; then"#));
    assert!(LAYERS.contains(
        r#"export NIX_REMOTE="''${NIX_REMOTE:-unix:///nix/var/nix/daemon-socket/socket}""#
    ));
    assert!(LAYERS.contains("agentbox-guest-init libkrun nix wait"));
    assert!(LAYERS.contains(r#"${pkgs.nix}/bin/nix store info --store "$NIX_REMOTE""#));
    assert!(LAYERS.contains("agentbox_nix_ready_marker"));

    let gate = LAYERS
        .find("AGENTBOX_LIBKRUN_NIX_OVERLAY")
        .expect("libkrun nix overlay gate should exist");
    let wait = LAYERS
        .find("agentbox-guest-init libkrun nix wait")
        .expect("nix wait should exist");
    let probe = LAYERS
        .find(r#"${pkgs.nix}/bin/nix store info --store "$NIX_REMOTE""#)
        .expect("real nix connectivity probe should exist");
    let exec = LAYERS
        .find(r#"exec ${pkgs.nix}/bin/nix "$@""#)
        .expect("real nix exec should exist");
    assert!(gate < wait);
    assert!(wait < probe);
    assert!(probe < exec);
}

#[test]
fn nix_wrapper_uses_real_nix_path_for_probe_and_exec() {
    for required in [
        "nixCommandCompat",
        "pkgs.writeShellScriptBin \"nix\"",
        "unset LD_PRELOAD",
        "unset NSS_WRAPPER_PASSWD",
        "unset NSS_WRAPPER_GROUP",
        r#"${pkgs.nix}/bin/nix store info --store "$NIX_REMOTE""#,
        r#"exec ${pkgs.nix}/bin/nix "$@""#,
    ] {
        assert!(LAYERS.contains(required), "missing {required}");
    }
}

#[test]
fn nix_wrapper_uses_marker_to_probe_connectivity_once_per_guest() {
    let marker = LAYERS
        .find("agentbox_nix_ready_marker")
        .expect("nix ready marker should exist");
    let probe = LAYERS
        .find(r#"${pkgs.nix}/bin/nix store info --store "$NIX_REMOTE""#)
        .expect("real nix connectivity probe should exist");
    let marker_write = LAYERS
        .find(r#": > "$agentbox_nix_ready_marker""#)
        .expect("marker write should exist");

    assert!(marker < probe);
    assert!(probe < marker_write);
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
