use crate::guest_init::components::home::identity::DevIdentity;
use crate::guest_init::components::shell::env::derive;
use std::path::PathBuf;

#[test]
fn internal_runtime_derives_parent_shell_environment_before_podman_prep() {
    let identity = DevIdentity::new(1234, 1235, PathBuf::from("/nix/store/fish/bin/fish"));
    let shell_env = derive(&identity, true);
    assert!(
        shell_env
            .vars
            .contains(&("HOME".to_owned(), "/home/dev".to_owned()))
    );
    assert!(
        shell_env
            .vars
            .contains(&("USER".to_owned(), "dev".to_owned()))
    );
    assert!(
        shell_env
            .vars
            .contains(&("LOGNAME".to_owned(), "dev".to_owned()))
    );
    assert!(
        shell_env
            .vars
            .contains(&("XDG_RUNTIME_DIR".to_owned(), "/run/user/1234".to_owned()))
    );
    assert!(
        shell_env
            .vars
            .iter()
            .any(|(key, value)| key == "PATH" && value.starts_with("/run/loftd/idmap-bin:"))
    );
    assert!(shell_env.vars.contains(&(
        "DOCKER_HOST".to_owned(),
        "unix:///run/user/1234/podman/podman.sock".to_owned()
    )));
    assert_eq!(shell_env.tmpdir, PathBuf::from("/home/dev/.cache/tmp"));
}

#[test]
fn internal_shell_environment_omits_docker_host_without_container_storage() {
    let identity = DevIdentity::new(1234, 1235, PathBuf::from("/nix/store/fish/bin/fish"));
    let shell_env = derive(&identity, false);

    assert!(!shell_env.vars.iter().any(|(key, _)| key == "DOCKER_HOST"));
    assert!(
        !shell_env
            .vars
            .iter()
            .any(|(key, _)| key == "XDG_RUNTIME_DIR")
    );
}
