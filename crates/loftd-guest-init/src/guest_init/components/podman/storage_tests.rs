const STORAGE_SOURCE: &str = include_str!("storage.rs");

#[test]
fn podman_storage_bootstrap_does_not_resize_btrfs_during_init() {
    assert!(
        !STORAGE_SOURCE.contains(r#""filesystem", "resize", "max""#),
        "nested-Podman storage bootstrap must not auto-grow btrfs during init"
    );
    assert!(
        !STORAGE_SOURCE.contains("continuing with existing container storage filesystem size"),
        "nested-Podman storage bootstrap must not keep the old resize warning path"
    );
}

#[test]
fn bind_store_bootstrap_no_longer_requires_btrfs_fstype_probe() {
    assert!(
        !STORAGE_SOURCE.contains("output_trimmed(\"findmnt\""),
        "bind-mode prep must not require findmnt filesystem probing"
    );
    assert!(
        !STORAGE_SOURCE.contains("requires a btrfs-backed host state directory"),
        "bind-mode prep must not require a btrfs-backed host state directory"
    );
}

#[test]
fn raw_disk_store_bootstrap_still_mounts_container_disk() {
    assert!(
        STORAGE_SOURCE.contains("disk::containers::ensure_mounted("),
        "raw-disk prep must keep mounting the persistent container-store disk"
    );
    assert!(STORAGE_SOURCE.contains("containers_disk_label"));
    assert!(STORAGE_SOURCE.contains("containers_disk_id"));
}

#[test]
fn podman_storage_bootstrap_does_not_recursively_chown_bind_store() {
    assert!(
        !STORAGE_SOURCE.contains("chown -R"),
        "bind-mode prep must not recursively chown arbitrary host state"
    );
}
