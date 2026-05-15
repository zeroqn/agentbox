use std::fs;

use super::{ensure_at, preload_contents, validate_allocator_lib};

const ALLOCATOR_LIB: &str = "/nix/store/eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee-graphene-hardened-malloc-14/lib/libhardened_malloc.so";

#[test]
fn preload_contents_is_single_allocator_line() {
    assert_eq!(
        preload_contents(ALLOCATOR_LIB),
        format!("{ALLOCATOR_LIB}\n")
    );
}

#[test]
fn ensure_at_writes_ld_nix_so_preload_file() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let path = temp.path().join("ld-nix.so.preload");

    ensure_at(&path, ALLOCATOR_LIB).expect("allocator preload should be written");

    assert_eq!(
        fs::read_to_string(path).unwrap(),
        format!("{ALLOCATOR_LIB}\n")
    );
}

#[test]
fn ensure_at_rejects_relative_allocator_path() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let path = temp.path().join("ld-nix.so.preload");

    let err = ensure_at(&path, "relative/lib/libhardened_malloc.so")
        .expect_err("relative allocator path should fail");

    assert!(
        format!("{err:#}").contains("must be an absolute path"),
        "unexpected error: {err:#}"
    );
}

#[test]
fn validate_allocator_lib_rejects_wrong_library_name() {
    let err = validate_allocator_lib("/nix/store/example/lib/libc.so")
        .expect_err("wrong library should fail");

    assert!(
        format!("{err:#}").contains("must point at libhardened_malloc.so"),
        "unexpected error: {err:#}"
    );
}
