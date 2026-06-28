const FLAKE_NIX: &str = include_str!("../../../flake.nix");
const ADR_0005_NEUTRAL_LOFTD_PREBUILT_ASSETS_MD: &str =
    include_str!("../../../docs/adr/0005-neutral-loftd-prebuilt-assets.md");
const LAYERS: &str = include_str!("../../../nix/image/layers.nix");
const CONTAINER_NIX: &str = include_str!("../../../nix/image/container.nix");
const IMAGE_CONFIG_NIX: &str = include_str!("../../../nix/image/config.nix");
const IMAGE_CHECKS_NIX: &str = include_str!("../../../nix/image/checks.nix");
const NIX_STORE_DB_CHECK_NIX: &str = include_str!("../../../nix/image/nix-store-db-check.nix");
const PINS_NIX: &str = include_str!("../../../nix/pins.nix");
const NIX_DEV_FLAKE_NIX: &str = include_str!("../../../nix/dev/flake.nix");
const SECCOMP_JSON_NIX: &str =
    include_str!("../../../nix/pkgs/container-lib-policy-seccomp-json.nix");
const AGENTBOX_RUST_NIX: &str = include_str!("../../../nix/pkgs/agentbox-rust.nix");
const AGENTBOX_PREBUILT_NIX: &str = include_str!("../../../nix/pkgs/agentbox-prebuilt.nix");
const LOFTD_PREBUILT_NIX: &str = include_str!("../../../nix/pkgs/loftd-prebuilt.nix");
const UPDATE_LOFTD_PREBUILT_SH: &str = include_str!("../../../scripts/update-loftd-prebuilt.sh");
const PUBLISH_RELEASE_YML: &str = include_str!("../../../.github/workflows/publish_release.yml");
const PUBLISH_IMAGE_YML: &str = include_str!("../../../.github/workflows/publish_image.yml");
const PUBLISH_DEV_IMAGE_YML: &str =
    include_str!("../../../.github/workflows/publish_dev_image.yml");
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

fn nix_top_level_attr_body<'a>(source: &'a str, attr_name: &str) -> &'a str {
    source
        .split(&format!("  {attr_name} = {{\n"))
        .nth(1)
        .and_then(|tail| tail.split("\n  };").next())
        .unwrap_or_else(|| panic!("{attr_name} attrset should exist"))
}

fn heredoc_bodies<'a>(source: &'a str, start: &str, end: &str) -> Vec<&'a str> {
    source
        .split(start)
        .skip(1)
        .filter_map(|tail| tail.split(end).next())
        .collect()
}

fn all_heredoc_bodies(source: &str) -> Vec<&str> {
    heredoc_bodies(source, "cat > release-notes.md <<EOF_NOTES", "EOF_NOTES")
}

fn assert_no_unescaped_backticks(source: &str) {
    let mut escaped = false;
    for character in source.chars() {
        if character == '`' && !escaped {
            panic!("unescaped backtick would execute as shell command substitution");
        }
        escaped = character == '\\' && !escaped;
        if character != '\\' {
            escaped = false;
        }
    }
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
fn flake_exposes_loftd_container_and_agentbox_container_outputs() {
    for required in [
        r#"loftdImage = mkImage "loftd";"#,
        r#"agentboxImage = mkImage "agentbox";"#,
        "agentbox = rustPackages.rustPackage;",
        "loftd = rustPackages.rustPackage;",
        "prebuiltLoftd = import ./nix/pkgs/loftd-prebuilt.nix",
        "loftd-prebuilt = prebuiltLoftd;",
        "container = loftdImage;",
        "agentbox-container = agentboxImage;",
        "container-nix-db-metadata = loftdImageChecks.imageConfigNixDbRefs;",
        "agentbox-container-nix-db-metadata = agentboxImageChecks.imageConfigNixDbRefs;",
    ] {
        assert!(FLAKE_NIX.contains(required), "missing {required}");
    }
    assert!(!FLAKE_NIX.contains("loftd-musl"));
    assert!(!FLAKE_NIX.contains("loftd-dev ="));
    assert!(
        NIX_DEV_FLAKE_NIX.contains("loftd-dev = rustPackages.rustPackage;"),
        "dev sub-flake should expose loftd-dev"
    );
}

#[test]
fn publish_image_workflows_publish_separate_loftd_and_agentbox_names() {
    for workflow in [PUBLISH_IMAGE_YML, PUBLISH_DEV_IMAGE_YML] {
        for required in [
            "strategy:",
            "fail-fast: false",
            "matrix:",
            "include:",
            "image_name: agentbox",
            "flake_attr: agentbox-container",
            "local_image: localhost/agentbox:latest",
            "init_binary: /bin/agentbox-guest-init",
            "image_name: loftd",
            "flake_attr: container",
            "local_image: localhost/loftd:latest",
            "init_binary: /bin/loftd-guest-init",
            r#"nix build ".#${{ matrix.flake_attr }}" -o "result-${{ matrix.image_name }}-container" --print-out-paths > "${{ matrix.image_name }}-container-path.txt""#,
            r#"docker load --input "$(cat "${{ matrix.image_name }}-container-path.txt")""#,
            r#"docker image inspect "${{ matrix.local_image }}" > /dev/null"#,
            r#"docker run --rm --entrypoint "${{ matrix.init_binary }}" "${{ matrix.local_image }}" --help > /dev/null"#,
            "target_image=ghcr.io/${owner}/${{ matrix.image_name }}",
            r#"docker tag "${{ matrix.local_image }}" "${{ steps.image_meta.outputs.target_image }}:${{ steps.image_meta.outputs.tag1 }}""#,
            r#"docker tag "${{ matrix.local_image }}" "${{ steps.image_meta.outputs.target_image }}:${{ steps.image_meta.outputs.tag2 }}""#,
            r#"docker push "${{ steps.image_meta.outputs.target_image }}:${{ steps.image_meta.outputs.tag1 }}""#,
            r#"docker push "${{ steps.image_meta.outputs.target_image }}:${{ steps.image_meta.outputs.tag2 }}""#,
        ] {
            assert!(workflow.contains(required), "missing {required}");
        }

        for forbidden in [
            "nix build .#agentbox-musl",
            "source_image=\"localhost/loftd:latest\"",
            "nix build .#agentbox-container -o result-agentbox-container --print-out-paths > agentbox-container-path.txt",
            "nix build .#container -o result-loftd-container --print-out-paths > loftd-container-path.txt",
            "docker load --input \"$(cat agentbox-container-path.txt)\"",
            "docker load --input \"$(cat loftd-container-path.txt)\"",
            "for target_image in",
            "docker push --all-tags",
            "steps.image_meta.outputs.agentbox_image",
            "steps.image_meta.outputs.loftd_image",
        ] {
            assert!(!workflow.contains(forbidden), "forbidden {forbidden}");
        }
    }

    for required in [
        r#"if [ "${GITHUB_REF_TYPE}" = "tag" ]; then"#,
        "tag1=${GITHUB_REF_NAME}",
        "tag1=latest",
        "tag2=sha-${short_sha}",
    ] {
        assert!(PUBLISH_IMAGE_YML.contains(required), "missing {required}");
    }

    for required in ["tag1=dev", "tag2=sha-${short_sha}"] {
        assert!(
            PUBLISH_DEV_IMAGE_YML.contains(required),
            "missing {required}"
        );
    }
}

#[test]
fn publish_release_uploads_agentbox_and_neutral_loftd_assets() {
    for required in [
        "nix build .#agentbox-musl -o result-agentbox-musl",
        "nix build .#loftd -o result-loftd",
        "agentbox_asset_name=\"agentbox-${arch}-unknown-linux-musl\"",
        "loftd_asset_name=\"loftd-${arch}-unknown-linux-gnu\"",
        "install -m 0755 result-agentbox-musl/bin/agentbox",
        "raw_loftd_path=\"result-loftd/bin/loftd\"",
        "refusing to publish loftd wrapper script",
        "refusing to publish non-ELF loftd asset",
        "readelf -h \"${raw_loftd_path}\" > /dev/null",
        "install -m 0755 \"${raw_loftd_path}\"",
        "neutral_loader=\"/lib64/ld-linux-x86-64.so.2\"",
        "neutral_loader=\"/lib/ld-linux-aarch64.so.1\"",
        "patchelf --set-interpreter",
        "--set-rpath \"\"",
        "grep -aEq '/nix/store/[0-9a-df-np-sv-z]{32}-'",
        "\"dist/${agentbox_asset_name}\" --help > /dev/null",
        "result-loftd/bin/loftd --help > /dev/null",
        "LOFTD_ASSET_PATH",
        "LOFTD_CHECKSUM_PATH",
        "neutral dynamically linked loftd ELF packaging input",
        "It is not a standalone portable binary",
        "Nix packaging patches ordinary ELF runtime dependencies",
        r"Prefer \`nix build .#loftd\`",
        r"\`nix build .#loftd-prebuilt\` once pinned",
    ] {
        assert!(PUBLISH_RELEASE_YML.contains(required), "missing {required}");
    }

    assert!(!PUBLISH_RELEASE_YML.contains(".#loftd-musl"));
    assert!(!PUBLISH_RELEASE_YML.contains(".loftd-wrapped"));
    assert!(
        !PUBLISH_RELEASE_YML
            .contains("install -m 0755 result-loftd/bin/loftd \"dist/${loftd_asset_name}\"")
    );
    assert!(!PUBLISH_RELEASE_YML.contains("\"dist/${loftd_asset_name}\" --help > /dev/null"));
}

#[test]
fn publish_release_notes_escape_markdown_backticks_for_shell_heredoc() {
    let bodies = all_heredoc_bodies(PUBLISH_RELEASE_YML);
    assert_eq!(
        bodies.len(),
        2,
        "expected tag and branch release notes heredocs"
    );
    let combined: String = bodies.join("\n");

    assert_no_unescaped_backticks(&combined);
    for required in [
        r"\`${{ steps.prep.outputs.agentbox_asset_name }}\`",
        r"\`${{ steps.prep.outputs.loftd_asset_name }}\`",
        r"\`nix build .#loftd\`",
        r"\`nix build .#loftd-prebuilt\`",
        r"\`ghcr.io/<repo-owner>/loftd\`",
        r"\`alpha\`",
        r"\`${{ steps.prep.outputs.sha_tag }}\`",
    ] {
        assert!(combined.contains(required), "missing {required}");
    }
}

#[test]
fn loftd_package_exposes_stable_raw_elf_payload_for_release_workflow() {
    for required in [
        "nativeBuildInputs = [ pkgs.makeWrapper ];",
        r#"mkdir -p "$out/libexec/loftd-helpers" "$out/lib/loftd""#,
        r#"ln -s ${pkgs.buildah}/bin/buildah "$out/libexec/loftd-helpers/buildah""#,
        r#"ln -s ${pkgs.btrfs-progs}/bin/btrfs "$out/libexec/loftd-helpers/btrfs""#,
        r#"ln -s ${pkgs.btrfs-progs}/bin/mkfs.btrfs "$out/libexec/loftd-helpers/mkfs.btrfs""#,
        r#"ln -s ${pkgs.util-linux}/bin/blkid "$out/libexec/loftd-helpers/blkid""#,
        r#"ln -s ${pkgs.passt}/bin/pasta "$out/libexec/loftd-helpers/pasta""#,
        r#"ln -s ${pkgs.passt}/bin/passt "$out/libexec/loftd-helpers/passt""#,
        "${pkgs.lib.getLib libkrun}/lib/libkrun.so*",
        "${pkgs.lib.getLib libkrunfw}/lib/libkrunfw.so*",
        r#"wrapProgram "$out/bin/agentbox""#,
    ] {
        assert!(AGENTBOX_RUST_NIX.contains(required), "missing {required}");
    }

    for removed in [
        r#"install -Dm755 "$out/bin/loftd" "$out/libexec/loftd""#,
        r#"wrapProgram "$out/bin/loftd""#,
    ] {
        assert!(
            !AGENTBOX_RUST_NIX.contains(removed),
            "still contains {removed}"
        );
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
        .split("imageContents =\n")
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
fn image_config_defines_loftd_and_agentbox_entrypoint_variants() {
    for required in [
        r#"loftd = {"#,
        r#""${agentboxMuslPackage}/bin/loftd-guest-init""#,
        r#"agentbox = {"#,
        r#""${agentboxMuslPackage}/bin/agentbox-guest-init""#,
        r#""default""#,
        r#"Entrypoint = variant.entrypoint;"#,
        r#"name = "localhost/${imageVariant}";"#,
    ] {
        assert!(
            IMAGE_CONFIG_NIX.contains(required) || CONTAINER_NIX.contains(required),
            "missing {required}"
        );
    }
    assert!(CONTAINER_NIX.contains("builtins.toJSON imageConfig"));
    assert!(!IMAGE_CONFIG_NIX.contains("agentbox-entrypoint"));
}

#[test]
fn image_env_exposes_guest_init_runtime_payloads() {
    for required in [
        r#"SHELL=${pkgs.fish}/bin/fish"#,
        r#"AGENTBOX_FISH_CONFIG_SOURCE=${configPayloads.fishConfig}/share/agentbox/fish/conf.d/agentbox-starship.fish"#,
        r#"AGENTBOX_STARSHIP_CONFIG_SOURCE=${configPayloads.starshipConfig}/share/agentbox/starship.toml"#,
        r#"AGENTBOX_NSS_WRAPPER_LIB=${pkgs.nss_wrapper}/lib/libnss_wrapper.so"#,
        r#"AGENTBOX_REAL_PODMAN=${layers.realPodmanBin}"#,
    ] {
        assert!(IMAGE_CONFIG_NIX.contains(required), "missing {required}");
    }
}

#[test]
fn image_exports_real_podman_path_for_guest_init_service_start() {
    assert!(LAYERS.contains(r#"realPodmanBin = "${podman}/bin/podman";"#));
    assert!(LAYERS.contains("realPodmanBin"));
    assert!(IMAGE_CONFIG_NIX.contains(r#"AGENTBOX_REAL_PODMAN=${layers.realPodmanBin}"#));
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
fn nix_agentbox_packages_provide_microvm_storage_helpers_at_runtime() {
    for required in [
        "runtimePath = pkgs.lib.makeBinPath [",
        "pkgs.buildah",
        "pkgs.btrfs-progs",
        "pkgs.fuse-overlayfs",
        "pkgs.util-linux",
        "\"PATH\"",
        "runtimeWrapperArgs",
        "wrapProgram \"$out/bin/agentbox\"",
        "loftd-helpers",
    ] {
        assert!(AGENTBOX_RUST_NIX.contains(required), "missing {required}");
    }

    for required in [
        "runtimeTools = [",
        "pkgs.buildah",
        "pkgs.btrfs-progs",
        "pkgs.fuse-overlayfs",
        "propagatedUserEnvPkgs = runtimeTools;",
        "pkgs.lib.makeBinPath runtimeTools",
    ] {
        assert!(
            AGENTBOX_PREBUILT_NIX.contains(required),
            "missing {required}"
        );
    }
}

#[test]
fn loftd_prebuilt_package_pins_and_patches_neutral_elf() {
    let loftd_pin = nix_top_level_attr_body(PINS_NIX, "loftdPrebuiltRelease");

    for required in [
        "owner = \"zeroqn\";",
        "repo = \"agentbox\";",
        "tag = \"sha-",
        "systems = {",
        "x86_64-linux = {",
        "asset = \"loftd-x86_64-unknown-linux-gnu\";",
        "hash = \"sha256-",
    ] {
        assert!(loftd_pin.contains(required), "missing {required}");
    }
    assert!(
        !loftd_pin.contains("systems = { };"),
        "loftd prebuilt pin should not remain in bootstrap-empty state"
    );

    for required in [
        "{ pkgs, pins, libkrun ? null, libkrunfw ? null }:",
        "loftdPrebuiltRelease = pins.loftdPrebuiltRelease;",
        "throw ''",
        "loftd-<arch>-unknown-linux-gnu",
        "pkgs.autoPatchelfHook",
        "pkgs.stdenv.cc.cc.lib",
        "pkgs.stdenv.cc.libc",
        "pkgs.buildah",
        "pkgs.btrfs-progs",
        "pkgs.fuse-overlayfs",
        "pkgs.util-linux",
        "pkgs.lib.getLib libkrun",
        "pkgs.lib.getLib libkrunfw",
        r#"magic="$(dd if="$src" bs=4 count=1"#,
        r#""7f454c46""#,
        r#"readelf -h "$src" >/dev/null"#,
        r#"install -Dm755 "$src" "$out/bin/loftd""#,
        r#"mkdir -p "$out/libexec/loftd-helpers" "$out/lib/loftd""#,
        r#"ln -s ${pkgs.buildah}/bin/buildah "$out/libexec/loftd-helpers/buildah""#,
        r#"ln -s ${pkgs.util-linux}/bin/blkid "$out/libexec/loftd-helpers/blkid""#,
        "Do not pin wrapper-script release assets",
        "after a neutral sha-* release is published",
        r#"mainProgram = "loftd";"#,
        "sourceProvenance = [ pkgs.lib.sourceTypes.binaryNativeCode ];",
    ] {
        assert!(LOFTD_PREBUILT_NIX.contains(required), "missing {required}");
    }

    for removed in [
        r#"install -Dm755 "$src" "$out/libexec/loftd""#,
        r#"makeWrapper "$out/libexec/loftd" "$out/bin/loftd""#,
        "runtimeWrapperArgs",
        r#""LD_LIBRARY_PATH""#,
    ] {
        assert!(
            !LOFTD_PREBUILT_NIX.contains(removed),
            "still contains {removed}"
        );
    }

    assert!(!UPDATE_LOFTD_PREBUILT_SH.contains("agentboxPrebuiltRelease"));
}

#[test]
fn loftd_prebuilt_adr_records_neutral_asset_decision() {
    for required in [
        "# Neutral loftd prebuilt release assets",
        "Status: accepted",
        "loftd-<arch>-unknown-linux-gnu",
        "not standalone portable executables",
        "autoPatchelfHook",
        "concrete
  `/nix/store/<hash>-...` references",
        "Use `unsafeDiscardReferences`: rejected",
    ] {
        assert!(
            ADR_0005_NEUTRAL_LOFTD_PREBUILT_ASSETS_MD.contains(required),
            "missing {required}"
        );
    }
}

#[test]
fn musl_package_keeps_loftd_host_out_of_static_output() {
    let musl_package = AGENTBOX_RUST_NIX
        .split("agentboxMuslPackage =")
        .nth(1)
        .and_then(|tail| tail.split("in\n{").next())
        .expect("agentboxMuslPackage should exist");

    for required in [
        "\"--package\"\n      \"agentbox-host\"",
        "\"--package\"\n      \"agentbox-guest-init\"",
        "\"--package\"\n      \"loftd-guest-init\"",
        "cargoBuildFlags = [",
        "cargoTestFlags = [",
    ] {
        assert!(musl_package.contains(required), "missing {required}");
    }

    assert!(
        !musl_package.contains("\"loftd\""),
        "static musl package must not build or expose the dynamic-only loftd host binary"
    );
}

#[test]
fn image_layers_include_guest_init_config_payloads() {
    assert!(LAYERS.contains("fishConfig"));
    assert!(LAYERS.contains("starshipConfig"));
    assert!(LAYERS.contains("agentboxMuslPackage"));
}

#[test]
fn podman_wrapper_waits_only_for_libkrun_container_storage() {
    let start = LAYERS
        .find("agentboxPodmanCommandCompat =")
        .expect("agentbox podman wrapper should exist");
    let end = LAYERS[start..]
        .find("loftdPodmanCommandCompat =")
        .map(|offset| start + offset)
        .expect("agentbox podman wrapper should end before loftd podman wrapper");
    let wrapper = &LAYERS[start..end];

    assert!(wrapper.contains(r#"if [ "''${AGENTBOX_LIBKRUN_CONTAINERS_STORAGE:-}" = "1" ]; then"#));
    let gate = wrapper
        .find("AGENTBOX_LIBKRUN_CONTAINERS_STORAGE")
        .expect("libkrun container storage gate should exist");
    let wait = wrapper
        .find("agentbox-guest-init libkrun podman wait")
        .expect("podman prep wait should exist");
    let exec = wrapper
        .find(r#"exec ${podman}/bin/podman "$@""#)
        .expect("real podman exec should exist");
    assert!(gate < wait);
    assert!(wait < exec);
    assert!(!wrapper.contains("agentbox-guest-init libkrun podman service-wait"));
    assert!(!wrapper.contains("podman system service"));
}

#[test]
fn docker_wrapper_waits_for_podman_and_execs_podman_compat() {
    let start = LAYERS
        .find("agentboxDockerCommandCompat =")
        .expect("agentbox docker wrapper should exist");
    let end = LAYERS[start..]
        .find("loftdDockerCommandCompat =")
        .map(|offset| start + offset)
        .expect("agentbox docker wrapper should end before loftd docker wrapper");
    let wrapper = &LAYERS[start..end];

    assert!(wrapper.contains(r#"pkgs.writeShellScriptBin "docker""#));
    assert!(wrapper.contains(r#"if [ "''${AGENTBOX_LIBKRUN_CONTAINERS_STORAGE:-}" = "1" ]; then"#));
    assert!(wrapper.contains("agentbox-guest-init libkrun podman service-wait"));
    assert!(wrapper.contains(r#"exec ${podman}/bin/podman "$@""#));
    assert!(!wrapper.contains("agentbox-guest-init libkrun podman wait"));
    assert!(!wrapper.contains("agentbox-guest-init libkrun docker"));
    assert!(!wrapper.contains("podman system service"));
    assert!(!wrapper.contains(r#"exec ${docker}/bin/docker "$@""#));
}

#[test]
fn docker_compose_wrapper_waits_for_podman_and_uses_docker_compose() {
    let start = LAYERS
        .find("agentboxDockerComposeCommandCompat =")
        .expect("agentbox docker-compose wrapper should exist");
    let end = LAYERS[start..]
        .find("loftdDockerComposeCommandCompat =")
        .map(|offset| start + offset)
        .expect("agentbox docker-compose wrapper should end before loftd compose wrapper");
    let wrapper = &LAYERS[start..end];

    assert!(wrapper.contains(r#"pkgs.writeShellScriptBin "docker-compose""#));
    assert!(wrapper.contains("agentbox-guest-init libkrun podman service-wait"));
    assert!(wrapper.contains(r#"exec ${pkgs.docker-compose}/bin/docker-compose "$@""#));
    assert!(!wrapper.contains("agentbox-guest-init libkrun podman wait"));
    assert!(!wrapper.contains("agentbox-guest-init libkrun docker"));
}

#[test]
fn loftd_wrappers_use_loftd_internal_wait_contracts() {
    for required in [
        "loftdNixCommandCompat = pkgs.writeShellScriptBin \"nix\"",
        "LOFTD_NIX_OVERLAY",
        "loftd-guest-init internal nix wait",
        "loftdPodmanCommandCompat = pkgs.writeShellScriptBin \"podman\"",
        "LOFTD_CONTAINERS_STORAGE",
        "loftd-guest-init internal podman wait",
        "loftdDockerCommandCompat = pkgs.writeShellScriptBin \"docker\"",
        "loftd-guest-init internal podman service-wait",
        "loftdDockerComposeCommandCompat = pkgs.writeShellScriptBin \"docker-compose\"",
        "loftd_nix_ready_marker",
    ] {
        assert!(LAYERS.contains(required), "missing {required}");
    }
}

#[test]
fn loftd_image_includes_root_only_as_dev_helper() {
    for required in [
        r#"loftdAsDevCommandCompat = pkgs.writeShellScriptBin "loftd-as-dev""#,
        "loftd-guest-init as-dev",
        "asDev = loftdAsDevCommandCompat",
        "loftdOnlyCommandCompat = pkgs.lib.optional (commandCompat.asDev != null) commandCompat.asDev",
        "++ imagePathPackages",
        "++ loftdOnlyCommandCompat",
    ] {
        assert!(LAYERS.contains(required), "missing {required}");
    }
}

#[test]
fn agentbox_image_does_not_select_loftd_as_dev_helper() {
    let agentbox_branch = LAYERS
        .split("asDev = loftdAsDevCommandCompat;")
        .nth(1)
        .expect("agentbox command compat branch should exist")
        .split("};")
        .next()
        .expect("agentbox command compat branch should end");

    assert!(agentbox_branch.contains("asDev = null"));
    assert!(!agentbox_branch.contains("loftdAsDevCommandCompat"));
}

#[test]
fn image_materializes_mimalloc_default_and_hardened_allocator_metadata() {
    for required in [
        r#"mimallocLib = "${pkgs.mimalloc}/lib/libmimalloc.so""#,
        r#"hardenedMallocLib = "${grapheneHardenedMalloc}/lib/libhardened_malloc.so""#,
        r#"printf '%s\n' '${layers.mimallocLib}' > ./etc/ld-nix.so.preload"#,
        "cat > ./etc/nix-allocator-libs <<EOF_NIX_ALLOCATOR_LIBS",
        "mimalloc=${layers.mimallocLib}",
        "hardened=${layers.hardenedMallocLib}",
        "chmod 0644 ./etc/ld-nix.so.preload",
        "chmod 0644 ./etc/nix-allocator-libs",
        r#"AGENTBOX_MIMALLOC_LIB=${layers.mimallocLib}"#,
        r#"AGENTBOX_GRAPHENE_HARDENED_MALLOC_LIB=${layers.hardenedMallocLib}"#,
        r#"LOFTD_MIMALLOC_LIB=${layers.mimallocLib}"#,
        r#"LOFTD_GRAPHENE_HARDENED_MALLOC_LIB=${layers.hardenedMallocLib}"#,
    ] {
        assert!(
            LAYERS.contains(required)
                || CONTAINER_NIX.contains(required)
                || IMAGE_CONFIG_NIX.contains(required),
            "missing {required}"
        );
    }
    assert!(!CONTAINER_NIX.contains("LD_PRELOAD=${layers.hardenedMallocLib}"));
    assert!(!CONTAINER_NIX.contains("LD_PRELOAD=${layers.mimallocLib}"));
}

#[test]
fn image_retains_graphene_hardened_malloc_for_hardened_mode() {
    for required in [
        "grapheneHardenedMalloc = pkgs.graphene-hardened-malloc.overrideAttrs",
        r#"version = "14";"#,
        r#"tag = "14";"#,
        r#"hash = "sha256-QUGDJyTnD5MuBUMlc4PZOZSAfevVUB6QbncVyXIAgb8=";"#,
        r#"hardenedMallocLib = "${grapheneHardenedMalloc}/lib/libhardened_malloc.so""#,
    ] {
        assert!(LAYERS.contains(required), "missing {required}");
    }
}

#[test]
fn image_uses_pinned_rmux_instead_of_tmux() {
    for required in [
        "rmuxPrebuilt = import ./nix/pkgs/rmux-prebuilt.nix",
        "rmux-prebuilt = rmuxPrebuilt;",
        "rmuxPrebuilt",
        "rmuxPrebuiltRelease = {",
        r#"owner = "Helvesec";"#,
        r#"repo = "rmux";"#,
    ] {
        assert!(
            FLAKE_NIX.contains(required)
                || LAYERS.contains(required)
                || CONTAINER_NIX.contains(required)
                || IMAGE_CHECKS_NIX.contains(required)
                || PINS_NIX.contains(required),
            "missing {required}"
        );
    }
    assert!(!LAYERS.contains("pkgs.tmux"));
    assert!(!CONTAINER_NIX.contains("./etc/tmux.conf"));
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
fn rust_tool_wrappers_mask_nix_loader_preload() {
    for required in [
        "rustcCommandCompat",
        r#"pkgs.writeShellScriptBin "rustc""#,
        "agentbox-empty-ld-nix-so-preload",
        "--ro-bind ${emptyLdNixSoPreload} /etc/ld-nix.so.preload",
        "--unsetenv LD_PRELOAD",
        "--unsetenv NSS_WRAPPER_PASSWD",
        "--unsetenv NSS_WRAPPER_GROUP",
        r#"${pkgs.rustc}/bin/rustc "$@""#,
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
        "container-nix-db-metadata = loftdImageChecks.imageConfigNixDbRefs;",
        "agentbox-container-nix-db-metadata = agentboxImageChecks.imageConfigNixDbRefs;",
    ] {
        assert!(FLAKE_NIX.contains(required), "missing {required}");
    }

    for required in [
        "storeRefsIn =",
        "pkgs.lib.splitString \"/nix/store/\" text",
        "imageConfigText = builtins.unsafeDiscardStringContext",
        "imageNixDbClosureInfo = pkgs.closureInfo",
        "rootPaths = layers.imageContents;",
        "imageNixDbStorePathsText = builtins.readFile",
        "missingImageConfigNixDbRefs = builtins.filter",
        "Missing from pkgs.closureInfo { rootPaths = layers.imageContents; }:",
        "It does not inspect, repair, or mutate the host Nix DB.",
        "cat ${missingRefsMessageFile} >&2",
    ] {
        assert!(IMAGE_CHECKS_NIX.contains(required), "missing {required}");
    }

    assert!(!LAYERS.contains("imageMetadataNixDbRoots"));

    for required in [
        "imageChecks = import ./checks.nix",
        "image = pkgs.dockerTools.buildLayeredImage",
        "config = builtins.fromJSON (",
        "builtins.unsafeDiscardStringContext (builtins.toJSON imageConfig)",
        "if imageChecks.missingImageConfigNixDbRefs != [ ] then",
        "builtins.throw imageChecks.missingRefsMessage",
        "image.overrideAttrs",
        "checking ${imageVariant} image config Nix DB metadata coverage",
        "test -e ${imageChecks.imageConfigNixDbRefs}/passed",
        "(old.buildCommand or \"\");",
    ] {
        assert!(CONTAINER_NIX.contains(required), "missing {required}");
    }
}

#[test]
fn image_includes_manual_nix_store_db_checker() {
    for required in [
        r#"toolName = if imageVariant == "loftd" then "loftd-nix-store-db-check" else "agentbox-nix-store-db-check""#,
        "nix path-info --all",
        "nix-store --verify-path",
        "! -name .links",
        "! -name '*.lock'",
        r#"runDir = if imageVariant == "loftd" then "/run/loftd" else "/run/agentbox""#,
        r#"libkrun_upper_dir="${runDir}/nix-disk/upper""#,
        "/store/",
        "/var/nix",
        "store object present in libkrun upperdir",
        "store object not found in libkrun upperdir; may come from lower image or another mounted view",
        "upperdir unavailable; overlay source evidence not inspected",
        "upper store subdir unavailable/empty",
        "store-layer evidence only; not root-cause evidence",
        "metadata-shadow context only",
        "no repair was attempted",
    ] {
        assert!(
            NIX_STORE_DB_CHECK_NIX.contains(required),
            "missing {required}"
        );
    }

    for forbidden in ["caused by upperdir", "lower image is at fault"] {
        assert!(
            !NIX_STORE_DB_CHECK_NIX.contains(forbidden),
            "misleading causal wording present: {forbidden}"
        );
    }

    for required in [
        "nixStoreDbCheck = import ./nix-store-db-check.nix { inherit pkgs imageVariant; };",
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
fn image_places_cargo_deny_in_tooling_layer_without_symposium() {
    let rust_toolchain = nix_list_body(LAYERS, "stableRustToolchainPackages");
    let tooling = nix_list_body(LAYERS, "toolingImagePackages");

    assert!(!rust_toolchain.contains("pkgs.cargo-deny"));
    assert!(tooling.contains("pkgs.cargo-deny"));
    assert!(!LAYERS.contains("symposium"));
}

#[test]
fn image_roots_musl_bin_output_exposed_by_image_path() {
    let c_toolchain_path = nix_list_body(LAYERS, "cToolchainPathPackages");

    assert!(c_toolchain_path.contains("pkgs.clang"));
    assert!(c_toolchain_path.contains("pkgs.gcc"));
    assert!(c_toolchain_path.contains("muslBin"));
    assert!(LAYERS.contains("imagePathPackages"));
    assert!(LAYERS.contains("loftdOnlyCommandCompat"));
    assert!(LAYERS.contains("++ imagePathPackages"));
    assert!(LAYERS.contains("muslBin = pkgs.lib.getBin pkgs.musl;"));
    assert!(LAYERS.contains("cToolchainImagePackages = cToolchainPathPackages ++ ["));
    assert!(LAYERS.contains("pkgs.musl"));
}

#[test]
fn image_does_not_wire_symposium_package_into_container_layers() {
    for retained_package_output in [
        "symposium = import ./nix/pkgs/symposium.nix",
        "symposium = symposium;",
    ] {
        assert!(
            FLAKE_NIX.contains(retained_package_output),
            "missing {retained_package_output}"
        );
    }

    assert!(!FLAKE_NIX.contains("symposium = packages.symposium;"));
    assert!(!FLAKE_NIX.contains("\n              symposium\n"));
    assert!(!CONTAINER_NIX.contains("symposium"));
    assert!(!IMAGE_CHECKS_NIX.contains("symposium"));
    assert!(!LAYERS.contains("symposium"));
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
