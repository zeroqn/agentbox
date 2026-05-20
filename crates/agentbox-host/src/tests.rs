const FLAKE_NIX: &str = include_str!("../../../flake.nix");
const LAYERS: &str = include_str!("../../../nix/image/layers.nix");
const CONTAINER_NIX: &str = include_str!("../../../nix/image/container.nix");
const IMAGE_CONFIG_NIX: &str = include_str!("../../../nix/image/config.nix");
const IMAGE_CHECKS_NIX: &str = include_str!("../../../nix/image/checks.nix");
const NIX_STORE_DB_CHECK_NIX: &str = include_str!("../../../nix/image/nix-store-db-check.nix");
const PINS_NIX: &str = include_str!("../../../nix/pins.nix");
const SECCOMP_JSON_NIX: &str =
    include_str!("../../../nix/pkgs/container-lib-policy-seccomp-json.nix");
const AGENTBOX_RUST_NIX: &str = include_str!("../../../nix/pkgs/agentbox-rust.nix");
const GUEST_DEFAULT_RUNTIME: &str =
    include_str!("../../agentbox-guest-init/src/guest_init/runtime/default.rs");
const GUEST_CONTAINER_RUNTIME: &str =
    include_str!("../../agentbox-guest-init/src/guest_init/runtime/container.rs");

fn nix_list_body<'a>(source: &'a str, list_name: &str) -> &'a str {
    source
        .split(&format!("{list_name} = ["))
        .nth(1)
        .and_then(|tail| tail.split("];").next())
        .unwrap_or_else(|| panic!("{list_name} list should exist"))
}

#[test]
fn flake_exposes_container_lib_seccomp_policy_package() {
    for required in [
        "containerLibPolicySeccompJson = import ./nix/pkgs/container-lib-policy-seccomp-json.nix",
        "container-lib-policy-seccomp-json = containerLibPolicySeccompJson;",
        "containerLibPolicySeccompJson",
    ] {
        assert!(FLAKE_NIX.contains(required), "missing {required}");
    }
}

#[test]
fn seccomp_policy_package_fetches_pinned_raw_container_libs_file() {
    for required in [
        "containerLibPolicySeccompJson = {",
        "owner = \"containers\";",
        "repo = \"container-libs\";",
        "path = \"common/pkg/seccomp/seccomp.json\";",
        "hash = \"sha256-m3VSAlFq7ktF2dQRq4AMIP5PevlxZqk7fwfVsWwaTs0=\";",
    ] {
        assert!(PINS_NIX.contains(required), "missing {required}");
    }

    for required in [
        "pkgs.fetchurl",
        "raw.githubusercontent.com/${pin.owner}/${pin.repo}/${pin.rev}/${pin.path}",
        "$out/share/containers/seccomp.json",
    ] {
        assert!(SECCOMP_JSON_NIX.contains(required), "missing {required}");
    }
}

#[test]
fn image_writes_global_seccomp_profile_config() {
    for required in [
        "./etc/containers",
        "cat > ./etc/containers/containers.conf <<'EOF_CONTAINERS_CONF'",
        "[containers]",
        "seccomp_profile = \"${containerLibPolicySeccompJson}/share/containers/seccomp.json\"",
        "chmod 0644 ./etc/containers/containers.conf",
    ] {
        assert!(CONTAINER_NIX.contains(required), "missing {required}");
    }
}

#[test]
fn image_includes_seccomp_policy_data_without_adding_it_to_path() {
    let image_contents = LAYERS
        .split("imageContents = imagePackages ++ [")
        .nth(1)
        .and_then(|tail| tail.split("];").next())
        .expect("imageContents should exist");
    assert!(image_contents.contains("containerLibPolicySeccompJson"));

    let image_path_start = LAYERS.find("imagePath =").expect("imagePath should exist");
    let image_path_end = LAYERS[image_path_start..]
        .find("agentboxImageMaxLayers")
        .map(|offset| image_path_start + offset)
        .expect("imagePath section should end before maxLayers");
    assert!(!LAYERS[image_path_start..image_path_end].contains("containerLibPolicySeccompJson"));
}

#[test]
fn image_entrypoint_invokes_guest_init_default_enter_directly() {
    assert!(IMAGE_CONFIG_NIX.contains(
        r#"Entrypoint = [ "${agentboxMuslPackage}/bin/agentbox-guest-init" "default" "enter" "--" ];"#
    ));
    assert!(CONTAINER_NIX.contains("config = imageConfig;"));
    assert!(!IMAGE_CONFIG_NIX.contains("agentbox-entrypoint"));
}

#[test]
fn image_env_exposes_guest_init_runtime_payloads() {
    for required in [
        r#"SHELL=${pkgs.fish}/bin/fish"#,
        r#"AGENTBOX_FISH_CONFIG_SOURCE=${configPayloads.fishConfig}/share/agentbox/fish/conf.d/agentbox-starship.fish"#,
        r#"AGENTBOX_STARSHIP_CONFIG_SOURCE=${configPayloads.starshipConfig}/share/agentbox/starship.toml"#,
        r#"AGENTBOX_NSS_WRAPPER_LIB=${pkgs.nss_wrapper}/lib/libnss_wrapper.so"#,
    ] {
        assert!(IMAGE_CONFIG_NIX.contains(required), "missing {required}");
    }
}

#[test]
fn default_guest_init_dispatches_libkrun_before_container_fallback() {
    for required in [
        "const LIBKRUN_NIX_OVERLAY_ENV",
        "const LIBKRUN_CONTAINERS_STORAGE_ENV",
        "fn should_dispatch_libkrun_from_env()",
        r#"runtime_dispatch_argv_for_exe(exe, "libkrun", command)"#,
        r#"runtime_dispatch_argv_for_exe(exe, "container", command)"#,
        r#""enter".to_owned()"#,
        r#""--".to_owned()"#,
    ] {
        assert!(
            GUEST_DEFAULT_RUNTIME.contains(required),
            "missing {required}"
        );
    }

    let dispatch = GUEST_DEFAULT_RUNTIME
        .find("should_dispatch_libkrun_from_env()")
        .expect("libkrun dispatch gate should exist");
    let fallback = GUEST_DEFAULT_RUNTIME
        .find("container_dispatch_argv")
        .expect("container fallback should exist");
    assert!(dispatch < fallback);

    assert!(
        !GUEST_CONTAINER_RUNTIME.contains("should_dispatch_libkrun_from_env()"),
        "explicit container runtime should not dispatch to libkrun"
    );
}

#[test]
fn nix_agentbox_binary_does_not_embed_libkrun_guest_init_image_path() {
    let removed_env = ["AGENTBOX", "LIBKRUN", "GUEST_INIT", "TARGET"].join("_");

    assert!(!AGENTBOX_RUST_NIX.contains(&removed_env));
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
fn docker_wrapper_waits_for_podman_and_execs_podman_compat() {
    let start = LAYERS
        .find("dockerCommandCompat =")
        .expect("docker wrapper should exist");
    let end = LAYERS[start..]
        .find("dockerComposeCommandCompat =")
        .map(|offset| start + offset)
        .expect("docker wrapper should end before compose wrapper");
    let wrapper = &LAYERS[start..end];

    assert!(wrapper.contains(r#"pkgs.writeShellScriptBin "docker""#));
    assert!(wrapper.contains(r#"if [ "''${AGENTBOX_LIBKRUN_CONTAINERS_STORAGE:-}" = "1" ]; then"#));
    assert!(wrapper.contains("agentbox-guest-init libkrun podman wait"));
    assert!(wrapper.contains(r#"exec ${podman}/bin/podman "$@""#));
    assert!(!wrapper.contains("agentbox-guest-init libkrun docker"));
    assert!(!wrapper.contains("DOCKER_HOST"));
    assert!(!wrapper.contains(r#"exec ${docker}/bin/docker "$@""#));
}

#[test]
fn docker_compose_wrapper_waits_for_podman_and_uses_docker_compose() {
    let start = LAYERS
        .find("dockerComposeCommandCompat =")
        .expect("docker-compose wrapper should exist");
    let end = LAYERS[start..]
        .find("sidecarProxyWrapper =")
        .map(|offset| start + offset)
        .expect("docker-compose wrapper should end before sidecar wrapper");
    let wrapper = &LAYERS[start..end];

    assert!(wrapper.contains(r#"pkgs.writeShellScriptBin "docker-compose""#));
    assert!(wrapper.contains("agentbox-guest-init libkrun podman wait"));
    assert!(wrapper.contains(r#"exec ${pkgs.docker-compose}/bin/docker-compose "$@""#));
    assert!(!wrapper.contains("agentbox-guest-init libkrun docker"));
}

#[test]
fn image_materializes_graphene_hardened_malloc_as_nix_loader_preload() {
    for required in [
        "grapheneHardenedMalloc = pkgs.graphene-hardened-malloc.overrideAttrs",
        r#"version = "14";"#,
        r#"tag = "14";"#,
        r#"hash = "sha256-QUGDJyTnD5MuBUMlc4PZOZSAfevVUB6QbncVyXIAgb8=";"#,
        r#"hardenedMallocLib = "${grapheneHardenedMalloc}/lib/libhardened_malloc.so""#,
        r#"printf '%s\n' '${layers.hardenedMallocLib}' > ./etc/ld-nix.so.preload"#,
        "chmod 0644 ./etc/ld-nix.so.preload",
        r#"AGENTBOX_GRAPHENE_HARDENED_MALLOC_LIB=${layers.hardenedMallocLib}"#,
    ] {
        assert!(
            LAYERS.contains(required)
                || CONTAINER_NIX.contains(required)
                || IMAGE_CONFIG_NIX.contains(required),
            "missing {required}"
        );
    }
    assert!(!CONTAINER_NIX.contains("LD_PRELOAD=${layers.hardenedMallocLib}"));
}

#[test]
fn image_materializes_system_tmux_defaults() {
    for required in [
        "cat > ./etc/tmux.conf <<'EOF_TMUX'",
        "bind-key | split-window -h",
        "bind-key - split-window -v",
        "bind-key h select-pane -L",
        "bind-key l select-pane -R",
        "bind-key j select-pane -D",
        "bind-key k select-pane -U",
        "chmod 0644 ./etc/tmux.conf",
    ] {
        assert!(CONTAINER_NIX.contains(required), "missing {required}");
    }
}

#[test]
fn image_includes_hardening_run_for_foreign_binary_allocator_opt_in() {
    for required in [
        r#"pkgs.writeShellScriptBin "hardening-run""#,
        "allocator_lib=${hardenedMallocLib}",
        r#"export LD_PRELOAD="$allocator_lib""#,
        r#"export LD_PRELOAD="$allocator_lib:$LD_PRELOAD""#,
        r#"exec "$@""#,
    ] {
        assert!(LAYERS.contains(required), "missing {required}");
    }
}

#[test]
fn rust_analyzer_wrapper_masks_nix_loader_preload() {
    for required in [
        "rustAnalyzerCommandCompat",
        r#"pkgs.writeShellScriptBin "rust-analyzer""#,
        "agentbox-empty-ld-nix-so-preload",
        "--ro-bind ${emptyLdNixSoPreload} /etc/ld-nix.so.preload",
        "--unsetenv LD_PRELOAD",
        "--unsetenv NSS_WRAPPER_PASSWD",
        "--unsetenv NSS_WRAPPER_GROUP",
        r#"${pkgs.rust-analyzer}/bin/rust-analyzer "$@""#,
    ] {
        assert!(LAYERS.contains(required), "missing {required}");
    }
}

#[test]
fn nix_wrapper_waits_and_probes_only_for_libkrun_nix_overlay() {
    assert!(LAYERS.contains(r#"if [ "''${AGENTBOX_LIBKRUN_NIX_OVERLAY:-}" = "1" ]; then"#));
    assert!(LAYERS.contains(
        r#"export NIX_REMOTE="''${NIX_REMOTE:-unix:///nix/var/nix/daemon-socket/socket}""#
    ));
    assert!(LAYERS.contains("agentbox-guest-init libkrun nix wait"));
    assert!(LAYERS.contains(r#"${pkgs.nix}/bin/nix store info --store "$NIX_REMOTE" --json"#));
    assert!(LAYERS.contains("agentbox_nix_ready_marker"));

    let gate = LAYERS
        .find("AGENTBOX_LIBKRUN_NIX_OVERLAY")
        .expect("libkrun nix overlay gate should exist");
    let wait = LAYERS
        .find("agentbox-guest-init libkrun nix wait")
        .expect("nix wait should exist");
    let probe = LAYERS
        .find(r#"${pkgs.nix}/bin/nix store info --store "$NIX_REMOTE" --json"#)
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
        r#"${pkgs.nix}/bin/nix store info --store "$NIX_REMOTE" --json"#,
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
        .find(r#"${pkgs.nix}/bin/nix store info --store "$NIX_REMOTE" --json"#)
        .expect("real nix connectivity probe should exist");
    let marker_write = LAYERS
        .find(r#": > "$agentbox_nix_ready_marker""#)
        .expect("marker write should exist");

    assert!(marker < probe);
    assert!(probe < marker_write);
}

#[test]
fn image_static_nix_db_metadata_check_is_flake_exposed() {
    for required in [
        "checks = systems.forAllSystems",
        "import ./nix/image/checks.nix",
        "container-nix-db-metadata = imageChecks.imageConfigNixDbRefs;",
    ] {
        assert!(FLAKE_NIX.contains(required), "missing {required}");
    }

    for required in [
        "builtins.unsafeDiscardStringContext metadataText",
        "dbClosure = pkgs.closureInfo",
        "rootPaths = layers.imageContents;",
        "grep -Eo '/nix/store/[0-9a-df-np-sv-z]{32}-[^/\":, ]+'",
        "comm -23 image-config-refs nix-db-roots-closure > missing-refs",
        "This check is diagnostic only. It did not repair or mutate Nix DB metadata.",
    ] {
        assert!(IMAGE_CHECKS_NIX.contains(required), "missing {required}");
    }
}

#[test]
fn image_includes_manual_nix_store_db_checker() {
    for required in [
        r#"pkgs.writeShellScriptBin "agentbox-nix-store-db-check""#,
        "nix path-info --all",
        "nix-store --verify-path",
        "! -name .links",
        "! -name '*.lock'",
        "no repair was attempted",
    ] {
        assert!(
            NIX_STORE_DB_CHECK_NIX.contains(required),
            "missing {required}"
        );
    }

    for required in [
        "nixStoreDbCheck = import ./nix-store-db-check.nix { inherit pkgs; };",
        "nixStoreDbCheck",
    ] {
        assert!(LAYERS.contains(required), "missing {required}");
    }
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
fn docker_wrapper_unsets_compat_env_before_execing_podman() {
    for required in [
        "dockerCommandCompat",
        "pkgs.writeShellScriptBin \"docker\"",
        "unset LD_PRELOAD",
        "unset NSS_WRAPPER_PASSWD",
        "unset NSS_WRAPPER_GROUP",
        r#"exec ${podman}/bin/podman "$@""#,
    ] {
        assert!(LAYERS.contains(required), "missing {required}");
    }
}

#[test]
fn image_places_cargo_deny_and_symposium_in_tooling_layer() {
    let rust_toolchain = nix_list_body(LAYERS, "stableRustToolchainPackages");
    let tooling = nix_list_body(LAYERS, "toolingImagePackages");

    assert!(!rust_toolchain.contains("pkgs.cargo-deny"));
    assert!(!rust_toolchain.contains("symposium"));
    assert!(tooling.contains("pkgs.cargo-deny"));
    assert!(tooling.contains("symposium"));
}

#[test]
fn image_wires_symposium_package_into_tooling_layer() {
    for required in [
        "symposium = import ./nix/pkgs/symposium.nix",
        "symposium = symposium;",
        "\n              symposium\n",
    ] {
        assert!(FLAKE_NIX.contains(required), "missing {required}");
    }

    assert!(CONTAINER_NIX.contains("piCodingAgent, symposium, rtkPrebuilt"));
    assert!(CONTAINER_NIX.contains("piCodingAgent symposium rtkPrebuilt"));
    assert!(LAYERS.contains("piCodingAgent, symposium, rtkPrebuilt"));
}

#[test]
fn image_includes_btrfs_progs_for_guest_bootstrap() {
    assert!(LAYERS.contains("pkgs.btrfs-progs"));
}

#[test]
fn image_includes_rootless_container_stacks_without_fuse_overlayfs() {
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
        "dockerCommandCompat",
        "dockerComposeCommandCompat",
        "pkgs.docker-compose",
    ] {
        assert!(LAYERS.contains(required), "missing {required}");
    }
    for forbidden in [
        "rootlessDockerImagePackages",
        "docker ? pkgs.docker",
        "pkgs.rootlesskit",
        "pkgs.slirp4netns",
        "pkgs.nftables",
        "dockerdRootlessCompat",
        "pkgs.writeShellScriptBin \"dockerd-rootless.sh\"",
        "agentbox-guest-init libkrun docker",
        r#"exec ${docker}/bin/docker "$@""#,
    ] {
        assert!(!LAYERS.contains(forbidden), "unexpected {forbidden}");
    }
    assert!(!LAYERS.contains("fuse-overlayfs"));
}
