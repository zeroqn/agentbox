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
fn container_store_bootstrap_does_not_probe_bind_store_fstype() {
    assert!(
        !STORAGE_SOURCE.contains("output_trimmed(\"findmnt\""),
        "container-store prep must not use the removed host-directory filesystem probe path"
    );
    assert!(
        !STORAGE_SOURCE.contains("requires a btrfs-backed host state directory"),
        "container-store prep must not require a btrfs-backed host state directory"
    );
}

#[test]
fn container_store_bootstrap_mounts_raw_disk() {
    assert!(
        STORAGE_SOURCE.contains("disk::containers::ensure_mounted("),
        "raw-disk prep must keep mounting the persistent container-store disk"
    );
    assert!(STORAGE_SOURCE.contains("containers_disk_label"));
    assert!(STORAGE_SOURCE.contains("containers_disk_id"));
}

#[test]
fn podman_storage_bootstrap_does_not_recursively_chown_container_store() {
    assert!(
        !STORAGE_SOURCE.contains("chown -R"),
        "container-store prep must not recursively chown arbitrary host state"
    );
}
