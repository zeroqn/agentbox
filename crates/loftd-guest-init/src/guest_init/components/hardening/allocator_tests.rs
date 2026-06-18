use std::fs;

use super::{
    AllocatorKind, ensure_at, parse_allocator_kind, preload_contents, select_allocator_lib,
    validate_allocator_lib,
};

const MIMALLOC_LIB: &str =
    "/nix/store/eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee-mimalloc-2.1.9/lib/libmimalloc.so";
const HARDENED_LIB: &str = "/nix/store/ffffffffffffffffffffffffffffffff-graphene-hardened-malloc-14/lib/libhardened_malloc.so";

#[test]
fn preload_contents_is_single_allocator_line() {
    assert_eq!(preload_contents(MIMALLOC_LIB), format!("{MIMALLOC_LIB}\n"));
    assert_eq!(preload_contents(HARDENED_LIB), format!("{HARDENED_LIB}\n"));
}

#[test]
fn ensure_at_writes_selected_allocator_preload() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let path = temp.path().join("ld-nix.so.preload");

    ensure_at(&path, MIMALLOC_LIB, AllocatorKind::Mimalloc)
        .expect("mimalloc preload should be written");
    assert_eq!(
        fs::read_to_string(&path).expect("preload should exist"),
        format!("{MIMALLOC_LIB}\n")
    );

    ensure_at(&path, HARDENED_LIB, AllocatorKind::Hardened)
        .expect("hardened preload should be written");
    assert_eq!(
        fs::read_to_string(&path).expect("preload should exist"),
        format!("{HARDENED_LIB}\n")
    );
}

#[test]
fn validate_allocator_lib_rejects_relative_allocator_path() {
    let err = validate_allocator_lib("relative/lib/libmimalloc.so", AllocatorKind::Mimalloc)
        .expect_err("relative allocator path should fail");

    assert!(format!("{err:#}").contains("must be an absolute path"));
}

#[test]
fn validate_allocator_lib_rejects_wrong_library_for_kind() {
    let err = validate_allocator_lib(HARDENED_LIB, AllocatorKind::Mimalloc)
        .expect_err("hardened malloc should not satisfy mimalloc");
    assert!(format!("{err:#}").contains("must point at mimalloc"));

    let err = validate_allocator_lib(MIMALLOC_LIB, AllocatorKind::Hardened)
        .expect_err("mimalloc should not satisfy hardened malloc");
    assert!(format!("{err:#}").contains("must point at hardened_malloc"));
}

#[test]
fn selector_defaults_to_mimalloc_and_accepts_hardened() {
    assert_eq!(
        parse_allocator_kind("").expect("empty selector"),
        AllocatorKind::Mimalloc
    );
    assert_eq!(
        parse_allocator_kind("mimalloc").expect("mimalloc selector"),
        AllocatorKind::Mimalloc
    );
    assert_eq!(
        parse_allocator_kind("hardened").expect("hardened selector"),
        AllocatorKind::Hardened
    );
    assert!(parse_allocator_kind("graphene").is_err());
}

#[test]
fn metadata_selection_defaults_to_mimalloc_path() {
    let metadata = format!("mimalloc={MIMALLOC_LIB}\nhardened={HARDENED_LIB}\n");

    let selected = select_allocator_lib(AllocatorKind::Mimalloc, Some(&metadata), None)
        .expect("metadata should parse");

    assert_eq!(selected.as_deref(), Some(MIMALLOC_LIB));
}

#[test]
fn metadata_selection_uses_hardened_path_when_requested() {
    let metadata = format!("mimalloc={MIMALLOC_LIB}\nhardened={HARDENED_LIB}\n");

    let selected = select_allocator_lib(AllocatorKind::Hardened, Some(&metadata), None)
        .expect("metadata should parse");

    assert_eq!(selected.as_deref(), Some(HARDENED_LIB));
}

#[test]
fn metadata_takes_precedence_over_env_fallback() {
    let fallback =
        "/nix/store/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-mimalloc-fallback/lib/libmimalloc.so";
    let metadata = format!("mimalloc={MIMALLOC_LIB}\n");

    let selected = select_allocator_lib(AllocatorKind::Mimalloc, Some(&metadata), Some(fallback))
        .expect("metadata should win");

    assert_eq!(selected.as_deref(), Some(MIMALLOC_LIB));
}

#[test]
fn missing_default_allocator_path_is_old_image_noop() {
    let selected = select_allocator_lib(AllocatorKind::Mimalloc, None, None)
        .expect("missing default path should stay compatible");

    assert_eq!(selected, None);
}

#[test]
fn explicit_hardened_without_path_fails() {
    let err = select_allocator_lib(AllocatorKind::Hardened, None, None)
        .expect_err("hardened selector requires a path");

    assert!(format!("{err:#}").contains("hardened requires hardened"));
}
