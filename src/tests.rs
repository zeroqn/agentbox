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
        "lowerdir=$agentbox_lower_dir,upperdir=$agentbox_upper_dir,workdir=$agentbox_work_dir",
        "${pkgs.nix}/bin/nix-daemon &",
        "export NIX_REMOTE=\"unix://$agentbox_socket\"",
        "libkrun in-guest nix-daemon socket is not accessible after dropping privileges",
    ] {
        assert!(ENTRYPOINT.contains(required), "missing {required}");
    }
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
