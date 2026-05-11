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

const ENTRYPOINT: &str = include_str!("../nix/image/entrypoint.nix");

#[test]
fn libkrun_entrypoint_branch_is_env_gated_and_does_not_require_proxy_host() {
    let gate = r#"if [ "''${AGENTBOX_LIBKRUN_NIX_OVERLAY:-}" = "1" ]; then
    bootstrap_libkrun_nix_overlay
  fi"#;
    assert!(ENTRYPOINT.contains(gate));

    let libkrun_branch_start = ENTRYPOINT
        .find(r#"if [ "''${AGENTBOX_LIBKRUN_NIX_OVERLAY:-}" = "1" ]; then"#)
        .expect("libkrun branch should exist");
    let proxy_branch_start = ENTRYPOINT
        .find(r#"agentbox_proxy_host="''${AGENTBOX_NIX_PROXY_HOST:-}""#)
        .expect("legacy proxy branch should remain for non-libkrun drop mode");
    let libkrun_branch = &ENTRYPOINT[libkrun_branch_start..proxy_branch_start];

    assert!(libkrun_branch.contains("bootstrap_libkrun_nix_overlay"));
    assert!(libkrun_branch.contains("NIX_REMOTE#unix://"));
    assert!(!libkrun_branch.contains("AGENTBOX_NIX_PROXY_HOST"));
}

#[test]
fn libkrun_entrypoint_preserves_lowerdir_mounts_overlay_and_starts_guest_daemon() {
    for required in [
        "mount --bind /nix \"$agentbox_lower_dir\"",
        "mount -o remount,bind,ro \"$agentbox_lower_dir\"",
        "btrfs filesystem resize max \"$agentbox_disk_mount\"",
        "mkdir -p \"$agentbox_upper_dir\" \"$agentbox_work_dir\" \"$agentbox_upper_dir/store\" \"$agentbox_upper_dir/var\"",
        "${pkgs.coreutils}/bin/cp -a --no-clobber \"$agentbox_lower_dir/var/.\" \"$agentbox_upper_dir/var/\"",
        "failed to preseed libkrun upperdir /nix/var from image lowerdir",
        "mkdir -p \"$agentbox_upper_dir/var/nix\"",
        "${pkgs.coreutils}/bin/chown :nixbld \"$agentbox_upper_dir/store\"",
        "chmod 1775 \"$agentbox_upper_dir/store\"",
        "chmod 0755 \"$agentbox_upper_dir/var\" \"$agentbox_upper_dir/var/nix\"",
        "lowerdir=$agentbox_lower_dir,upperdir=$agentbox_upper_dir,workdir=$agentbox_work_dir",
        "${pkgs.nix}/bin/nix-daemon &",
        "export NIX_REMOTE=\"unix://$agentbox_socket\"",
        "libkrun in-guest nix-daemon socket is not accessible after dropping privileges",
    ] {
        assert!(ENTRYPOINT.contains(required), "missing {required}");
    }
}

#[test]
fn libkrun_entrypoint_preseeds_store_and_var_nix_before_overlay_mount() {
    let resize = ENTRYPOINT
        .find("btrfs filesystem resize max \"$agentbox_disk_mount\"")
        .expect("btrfs resize should happen before upperdir preseed");
    let preseed_mkdir = ENTRYPOINT
        .find("mkdir -p \"$agentbox_upper_dir\" \"$agentbox_work_dir\" \"$agentbox_upper_dir/store\" \"$agentbox_upper_dir/var\"")
        .expect("upperdir /store and /var preseed mkdir should exist");
    let var_copy = ENTRYPOINT
        .find("${pkgs.coreutils}/bin/cp -a --no-clobber \"$agentbox_lower_dir/var/.\" \"$agentbox_upper_dir/var/\"")
        .expect("image /nix/var should be copied into upperdir before overlay mount");
    let var_nix_mkdir = ENTRYPOINT
        .find("mkdir -p \"$agentbox_upper_dir/var/nix\"")
        .expect("upperdir /var/nix preseed mkdir should exist");
    let preseed_chmod = ENTRYPOINT
        .find("chmod 1775 \"$agentbox_upper_dir/store\"")
        .expect("upperdir /store chmod should exist");
    let var_nix_chmod = ENTRYPOINT
        .find("chmod 0755 \"$agentbox_upper_dir/var\" \"$agentbox_upper_dir/var/nix\"")
        .expect("upperdir /var/nix preseed chmod should exist");
    let preseed_chown = ENTRYPOINT
        .find("${pkgs.coreutils}/bin/chown :nixbld \"$agentbox_upper_dir/store\"")
        .expect("upperdir /store group ownership should be set to nixbld");
    let overlay_mount = ENTRYPOINT
        .find("${pkgs.util-linux}/bin/mount -t overlay overlay")
        .expect("overlay mount should exist");
    let socket_mkdir = ENTRYPOINT
        .find("mkdir -p /nix/var/nix/daemon-socket")
        .expect("daemon socket directory mkdir should exist");
    let daemon_start = ENTRYPOINT
        .find("${pkgs.nix}/bin/nix-daemon &")
        .expect("nix-daemon start should exist");

    assert!(
        resize < preseed_mkdir,
        "btrfs resize should happen before upperdir /var/nix preseed"
    );
    assert!(
        preseed_mkdir < preseed_chmod,
        "upperdir /var mkdir should happen before chmod"
    );
    assert!(
        preseed_mkdir < var_copy,
        "upperdir /var should exist before copying image /nix/var"
    );
    assert!(
        var_copy < var_nix_mkdir,
        "image /nix/var copy should happen before ensuring upperdir /var/nix exists"
    );
    assert!(
        var_nix_mkdir < preseed_chmod,
        "upperdir /var/nix mkdir should happen before chmod"
    );
    assert!(
        preseed_chown < preseed_chmod,
        "upperdir /store group ownership should be set before final chmod"
    );
    assert!(
        preseed_chown < overlay_mount,
        "upperdir /store group ownership should be set before overlay mount"
    );
    assert!(
        preseed_chmod < overlay_mount,
        "upperdir /store chmod should happen before overlay mount"
    );
    assert!(
        var_nix_chmod < overlay_mount,
        "upperdir /var/nix chmod should happen before overlay mount"
    );
    assert!(
        var_copy < overlay_mount,
        "image /nix/var copy should happen before overlay mount"
    );
    assert!(
        overlay_mount < socket_mkdir,
        "daemon socket directory should be created after overlay mount"
    );
    assert!(
        socket_mkdir < daemon_start,
        "daemon socket directory should be created before nix-daemon starts"
    );
    assert!(
        !ENTRYPOINT.contains("chown -R :nixbld"),
        "libkrun bootstrap must not recursively chown /nix/store children"
    );
}

#[test]
fn libkrun_entrypoint_has_fail_fast_diagnostics_for_expected_failures() {
    for diagnostic in [
        "required tool '$tool_name' is not available",
        "libkrun /nix btrfs disk not found",
        "failed to preserve image /nix lowerdir",
        "failed to mount libkrun /nix btrfs disk",
        "failed to mount libkrun overlay at /nix",
        "nix-daemon exited before creating",
        "nix-daemon did not create",
    ] {
        assert!(ENTRYPOINT.contains(diagnostic), "missing {diagnostic}");
    }
}

#[test]
fn image_includes_btrfs_progs_for_guest_bootstrap() {
    let layers = include_str!("../nix/image/layers.nix");
    assert!(layers.contains("pkgs.btrfs-progs"));
}
