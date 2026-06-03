use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::PathBuf;

use crate::guest_init::components::home::identity::DevIdentity;
use crate::guest_init::components::home::root::{ensure_home_dirs, home_dirs, nix_cache_dir};

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

#[test]
fn home_setup_makes_nix_cache_private() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let metadata = std::fs::metadata(temp.path()).expect("tempdir metadata should be readable");
    let identity = DevIdentity {
        uid: metadata.uid(),
        gid: metadata.gid(),
        home: temp.path().join("home"),
        shell: PathBuf::from("fish"),
    };

    ensure_home_dirs(&identity).expect("home dirs should be created");

    assert_eq!(
        std::fs::metadata(identity.home.join(".cache/nix"))
            .expect("nix cache metadata should be readable")
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
}
