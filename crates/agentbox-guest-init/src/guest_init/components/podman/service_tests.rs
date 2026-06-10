use super::PodmanServiceLock;
use crate::guest_init::components::home::identity::DevIdentity;
use crate::guest_init::components::podman::service::{
    PodmanServicePaths, REAL_PODMAN_ENV, command_plan, docker_host_uri, socket_is_live,
    wait_for_socket,
};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::time::Duration;
use tempfile::tempdir;

#[test]
fn podman_service_paths_are_uid_derived() {
    let identity = DevIdentity::new(1234, 1235, PathBuf::from("/bin/fish"));
    let paths = PodmanServicePaths::for_identity(&identity);

    assert_eq!(paths.runtime_dir, PathBuf::from("/run/user/1234"));
    assert_eq!(
        paths.socket_path,
        PathBuf::from("/run/user/1234/podman/podman.sock")
    );
    assert_eq!(paths.socket_uri, "unix:///run/user/1234/podman/podman.sock");
    assert_eq!(docker_host_uri(&identity), paths.socket_uri);
}

#[test]
fn podman_service_command_uses_real_binary_and_rootless_env() {
    let temp = tempdir().unwrap();
    let podman = executable_fixture(temp.path().join("real-podman"), "#!/bin/sh\nexit 0\n");
    let old_path = std::env::var_os("PATH");
    let old_real = std::env::var_os(REAL_PODMAN_ENV);
    unsafe {
        std::env::set_var(REAL_PODMAN_ENV, &podman);
        std::env::set_var("PATH", "/nix/store/wrappers/bin:/nix/store/podman/bin");
    }

    let identity = DevIdentity::new(1234, 1235, PathBuf::from("/bin/fish"));
    let paths = PodmanServicePaths::for_identity(&identity);
    let plan = command_plan(&identity, &paths).unwrap();

    assert_eq!(plan.program, podman);
    assert_eq!(
        plan.args,
        vec![
            "system",
            "service",
            "--time=0",
            "unix:///run/user/1234/podman/podman.sock"
        ]
    );
    assert_env(&plan.env, "USER", "dev");
    assert_env(&plan.env, "HOME", "/home/dev");
    assert_env(&plan.env, "SHELL", "/bin/fish");
    assert_env(&plan.env, "XDG_RUNTIME_DIR", "/run/user/1234");
    assert_env(&plan.env, "XDG_CONFIG_HOME", "/home/dev/.config");
    assert_env(&plan.env, "XDG_DATA_HOME", "/home/dev/.local/share");
    assert_env(&plan.env, "XDG_STATE_HOME", "/home/dev/.local/state");
    assert_env(&plan.env, "XDG_CACHE_HOME", "/home/dev/.cache");
    assert_env(&plan.env, "TMPDIR", "/home/dev/.cache/tmp");
    assert!(
        plan.env
            .iter()
            .any(|(key, value)| key == "PATH" && value.starts_with("/run/agentbox/wrappers/bin:"))
    );

    restore_env(REAL_PODMAN_ENV, old_real);
    restore_env("PATH", old_path);
}

#[test]
fn podman_service_refuses_agentbox_compat_wrapper() {
    for (name, body) in [
        (
            "prep-wait",
            "#!/bin/sh\nagentbox-guest-init libkrun podman wait\nexec /real/podman \"$@\"\n",
        ),
        (
            "service-wait",
            "#!/bin/sh\nagentbox-guest-init libkrun podman service-wait\nexec /real/podman \"$@\"\n",
        ),
    ] {
        let temp = tempdir().unwrap();
        let wrapper = executable_fixture(temp.path().join("podman"), body);
        let old_real = std::env::var_os(REAL_PODMAN_ENV);
        unsafe {
            std::env::set_var(REAL_PODMAN_ENV, &wrapper);
        }

        let identity = DevIdentity::new(1234, 1235, PathBuf::from("/bin/fish"));
        let paths = PodmanServicePaths::for_identity(&identity);
        let err = command_plan(&identity, &paths).unwrap_err();

        assert!(
            err.to_string().contains("compatibility wrapper"),
            "{name} wrapper should be rejected"
        );
        restore_env(REAL_PODMAN_ENV, old_real);
    }
}

#[test]
fn podman_socket_probe_detects_live_unix_listener() {
    let temp = tempdir().unwrap();
    let socket = temp.path().join("podman.sock");
    let _listener = UnixListener::bind(&socket).unwrap();

    assert!(socket_is_live(&socket));
    wait_for_socket(&socket, Duration::from_millis(10)).unwrap();
}

#[test]
fn podman_socket_wait_times_out_for_missing_socket() {
    let temp = tempdir().unwrap();
    let socket = temp.path().join("missing.sock");
    let err = wait_for_socket(&socket, Duration::from_millis(0)).unwrap_err();

    assert!(
        err.to_string()
            .contains("timed out waiting for Podman API socket")
    );
    assert!(err.to_string().contains("missing.sock"));
}

#[test]
fn podman_service_lock_times_out_instead_of_blocking_forever() {
    let temp = tempdir().unwrap();
    let identity = DevIdentity::new(1234, 1235, PathBuf::from("/bin/fish"));
    let paths = PodmanServicePaths::from_runtime_dir(temp.path().join("runtime"));
    fs::create_dir_all(&paths.socket_dir).unwrap();

    let _held = PodmanServiceLock::acquire_until(
        &identity,
        &paths,
        std::time::Instant::now() + Duration::from_secs(1),
    )
    .unwrap();
    let err = match PodmanServiceLock::acquire_until(
        &identity,
        &paths,
        std::time::Instant::now() + Duration::from_millis(0),
    ) {
        Ok(_) => panic!("second lock acquisition should time out"),
        Err(err) => err,
    };

    assert!(
        err.to_string()
            .contains("timed out waiting for Podman API service lock")
    );
}

fn executable_fixture(path: PathBuf, contents: &str) -> PathBuf {
    fs::write(&path, contents).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    path
}

fn assert_env(env: &[(String, String)], key: &str, value: &str) {
    assert!(
        env.iter().any(
            |(candidate_key, candidate_value)| candidate_key == key && candidate_value == value
        ),
        "missing {key}={value} in {env:?}"
    );
}

fn restore_env(key: &str, value: Option<std::ffi::OsString>) {
    unsafe {
        match value {
            Some(value) => std::env::set_var(key, value),
            None => std::env::remove_var(key),
        }
    }
}
