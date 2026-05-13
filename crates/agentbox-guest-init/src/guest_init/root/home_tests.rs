use crate::guest_init::root::home::DevIdentity;
use std::path::PathBuf;

#[test]
fn dev_identity_uses_fixed_home_and_selected_shell() {
    let identity = DevIdentity::new(501, 20, PathBuf::from("/bin/fish"));
    assert_eq!(identity.home, PathBuf::from("/home/dev"));
    assert_eq!(identity.shell, PathBuf::from("/bin/fish"));
}
