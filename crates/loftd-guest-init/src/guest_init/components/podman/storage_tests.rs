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
