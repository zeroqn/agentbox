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
fn bind_store_btrfs_validation_accepts_only_btrfs() {
    super::validate_bind_store_fstype(Some("btrfs")).expect("btrfs should pass");

    let ext4 = super::validate_bind_store_fstype(Some("ext4")).expect_err("ext4 should fail");
    let missing = super::validate_bind_store_fstype(None).expect_err("missing fstype should fail");

    assert!(ext4.to_string().contains("not 'btrfs'"));
    assert!(missing.to_string().contains("could not be detected"));
}

#[test]
fn podman_storage_bootstrap_does_not_recursively_chown_bind_store() {
    assert!(
        !STORAGE_SOURCE.contains("chown -R"),
        "bind-mode prep must not recursively chown arbitrary host state"
    );
}
