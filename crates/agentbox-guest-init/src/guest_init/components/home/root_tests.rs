use std::path::PathBuf;

use crate::guest_init::components::home::identity::DevIdentity;
use crate::guest_init::components::home::root::{home_dirs, nix_cache_dir};

#[test]
fn home_dirs_cover_cache_parent_but_leave_nix_cache_to_recursive_repair() {
    let identity = DevIdentity::new(1000, 1000, PathBuf::from("fish"));

    let dirs = home_dirs(&identity);

    assert!(dirs.contains(&PathBuf::from("/home/dev/.cache")));
    assert!(dirs.contains(&PathBuf::from("/home/dev/.cache/tmp")));
    assert!(
        !dirs.contains(&PathBuf::from("/home/dev/.cache/nix")),
        ".cache/nix must be handled by the symlink-safe recursive repair path"
    );
    assert_eq!(
        nix_cache_dir(&identity),
        PathBuf::from("/home/dev/.cache/nix")
    );
}
