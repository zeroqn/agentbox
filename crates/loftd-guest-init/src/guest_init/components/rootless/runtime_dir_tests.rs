use crate::guest_init::components::rootless::runtime_dir::ensure_user_runtime_dir;

#[test]
fn rootless_runtime_dir_helper_targets_run_user_parent() {
    let source = include_str!("runtime_dir.rs");
    assert!(source.contains("/run/user/{}"));
    assert!(source.contains("fs::chown(&run_dir, identity.uid, identity.gid)"));
    assert!(source.contains("fs::chmod(&run_dir, 0o700)"));
    let _ = ensure_user_runtime_dir;
}
