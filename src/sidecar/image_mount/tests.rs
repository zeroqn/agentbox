use super::*;
use crate::DEFAULT_IMAGE;
use std::fs;

#[test]
fn build_podman_image_mount_args_uses_direct_mode_by_default_path() {
    let args = build_podman_image_mount_args(DEFAULT_IMAGE, PodmanImageMountMode::Direct);
    assert_eq!(
        args,
        vec![
            "image".to_owned(),
            "mount".to_owned(),
            DEFAULT_IMAGE.to_owned(),
        ]
    );
}

#[test]
fn build_podman_image_mount_args_supports_unshare_fallback_mode() {
    let args = build_podman_image_mount_args(DEFAULT_IMAGE, PodmanImageMountMode::Unshare);
    assert_eq!(
        args,
        vec![
            "unshare".to_owned(),
            "podman".to_owned(),
            "image".to_owned(),
            "mount".to_owned(),
            DEFAULT_IMAGE.to_owned(),
        ]
    );
}

#[test]
fn build_podman_image_unmount_args_uses_direct_mode_by_default_path() {
    let args = build_podman_image_unmount_args(DEFAULT_IMAGE, PodmanImageMountMode::Direct);
    assert_eq!(
        args,
        vec![
            "image".to_owned(),
            "unmount".to_owned(),
            DEFAULT_IMAGE.to_owned(),
        ]
    );
}

#[test]
fn build_podman_image_unmount_args_supports_unshare_fallback_mode() {
    let args = build_podman_image_unmount_args(DEFAULT_IMAGE, PodmanImageMountMode::Unshare);
    assert_eq!(
        args,
        vec![
            "unshare".to_owned(),
            "podman".to_owned(),
            "image".to_owned(),
            "unmount".to_owned(),
            DEFAULT_IMAGE.to_owned(),
        ]
    );
}

#[test]
fn resolve_sidecar_lowerdir_prefers_nested_nix_directory() {
    let dir = tempfile::tempdir().expect("tempdir should be created");
    let mount = dir.path();
    fs::create_dir_all(mount.join("nix")).expect("nested nix dir should be created");
    fs::create_dir_all(mount.join("store")).expect("root store dir should be created");

    let lowerdir = super::super::resolve_sidecar_lowerdir(mount).expect("lowerdir should resolve");
    assert_eq!(lowerdir, mount.join("nix"));
}

#[test]
fn resolve_sidecar_lowerdir_falls_back_to_mount_root_when_store_exists() {
    let dir = tempfile::tempdir().expect("tempdir should be created");
    let mount = dir.path();
    fs::create_dir_all(mount.join("store")).expect("root store dir should be created");

    let lowerdir = super::super::resolve_sidecar_lowerdir(mount).expect("lowerdir should resolve");
    assert_eq!(lowerdir, mount);
}

#[test]
fn resolve_sidecar_lowerdir_fails_when_nix_paths_are_missing() {
    let dir = tempfile::tempdir().expect("tempdir should be created");
    let mount = dir.path();

    let err = super::super::resolve_sidecar_lowerdir(mount).expect_err("lowerdir should fail");
    assert!(err
        .to_string()
        .contains(&mount.join("nix").display().to_string()));
    assert!(err
        .to_string()
        .contains(&mount.join("store").display().to_string()));
}
