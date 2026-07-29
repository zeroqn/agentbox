const FLAKE_NIX: &str = include_str!("../../../flake.nix");
const LAYERS: &str = include_str!("../../../nix/image/layers.nix");
const IMAGE_CONFIG_NIX: &str = include_str!("../../../nix/image/config.nix");
const NIX_STORE_DB_CHECK_NIX: &str = include_str!("../../../nix/image/nix-store-db-check.nix");
const AGENTBOX_RUST_NIX: &str = include_str!("../../../nix/pkgs/agentbox-rust.nix");

fn nix_top_level_attr_body<'a>(source: &'a str, attr_name: &str) -> &'a str {
    source
        .split(&format!("  {attr_name} = {{\n"))
        .nth(1)
        .and_then(|tail| tail.split("\n  };\n").next())
        .unwrap_or_else(|| panic!("{attr_name} attrset should exist"))
}

#[test]
fn flake_keeps_local_agentbox_outputs() {
    for required in [
        "agentbox = rustPackages.rustPackage;",
        "agentbox-container = agentboxImage;",
        "agentbox-ci-sccache = rustPackagesCiSccache.rustPackage;",
        "agentbox-musl-ci-sccache = rustPackagesCiSccache.agentboxMuslPackage;",
        "agentbox-container-ci-sccache = agentboxImageCiSccache;",
        "agentbox-container-nix-db-metadata = agentboxImageChecks.imageConfigNixDbRefs;",
    ] {
        assert!(FLAKE_NIX.contains(required), "missing {required}");
    }
}

#[test]
fn image_config_keeps_agentbox_entrypoint() {
    let agentbox = nix_top_level_attr_body(IMAGE_CONFIG_NIX, "agentbox");
    assert!(agentbox.contains(r#""${agentboxMuslPackage}/bin/agentbox-guest-init""#));
    assert!(agentbox.contains(r#""default""#));
    assert!(agentbox.contains(r#""enter""#));
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
        "wrapProgram \"$out/bin/agentbox\"",
    ] {
        assert!(AGENTBOX_RUST_NIX.contains(required), "missing {required}");
    }
}

#[test]
fn agentbox_image_does_not_select_loftd_as_dev_helper() {
    let agentbox_branch = LAYERS
        .split(r#"if imageVariant == "loftd" then"#)
        .nth(1)
        .and_then(|tail| tail.split("else").nth(1))
        .expect("agentbox command compat branch should exist");

    assert!(agentbox_branch.contains("asDev = null"));
    assert!(!agentbox_branch.contains("loftdAsDevCommandCompat"));
}

#[test]
fn agentbox_manual_nix_store_db_checker_keeps_agentbox_identity() {
    assert!(NIX_STORE_DB_CHECK_NIX.contains(
        r#"toolName = if imageVariant == "loftd" then "loftd-nix-store-db-check" else "agentbox-nix-store-db-check""#,
    ));
    assert!(
        NIX_STORE_DB_CHECK_NIX.contains(
            r#"runDir = if imageVariant == "loftd" then "/run/loftd" else "/run/agentbox""#,
        )
    );
}
