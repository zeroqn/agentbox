use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::symlink;
use std::path::PathBuf;

use super::{
    ContainerEnterOperation, HOST_GID_ENV, HOST_UID_ENV, ProcessIds, build_nss_wrapper_plan,
    derive_identity_plan, materialize_writable_dir, normal_shell_environment,
    planned_enter_operations,
};
use crate::guest_init::cli::EnterCommand;
use crate::guest_init::components::env::ENTER_AS_ROOT_ENV;

struct TestEnv(BTreeMap<String, String>);

impl TestEnv {
    fn new(vars: &[(&str, &str)]) -> Self {
        Self(
            vars.iter()
                .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
                .collect(),
        )
    }
}

impl super::EnvSource for TestEnv {
    fn var(&self, name: &str) -> Option<String> {
        self.0.get(name).cloned()
    }
}

#[test]
fn default_container_enter_command_is_fish_login_shell() {
    let enter = EnterCommand { command: vec![] };

    assert_eq!(enter.resolved_command(), ["fish", "-l"]);
}

#[test]
fn explicit_container_enter_command_is_preserved() {
    let enter = EnterCommand {
        command: vec!["bash".to_owned(), "-lc".to_owned(), "true".to_owned()],
    };

    assert_eq!(enter.resolved_command(), ["bash", "-lc", "true"]);
}

#[test]
fn identity_plan_drops_root_interactive_fish_to_host_identity() {
    let command = vec!["/nix/store/fish/bin/fish".to_owned(), "-l".to_owned()];
    let plan = derive_identity_plan(
        &command,
        ProcessIds { uid: 0, gid: 0 },
        &TestEnv::new(&[(HOST_UID_ENV, "1001"), (HOST_GID_ENV, "1002")]),
    )
    .expect("interactive fish should derive identity");

    assert!(plan.drop_to_dev);
    assert_eq!(plan.identity.uid, 1001);
    assert_eq!(plan.identity.gid, 1002);
    assert_eq!(plan.identity.home, PathBuf::from("/home/dev"));
    assert!(plan.identity.shell.ends_with("fish"));
}

#[test]
fn identity_plan_requires_host_identity_when_root_must_drop() {
    let command = vec!["fish".to_owned(), "-l".to_owned()];
    let err = derive_identity_plan(&command, ProcessIds { uid: 0, gid: 0 }, &TestEnv::new(&[]))
        .expect_err("root fish login shell should require host uid/gid before dropping");

    assert!(
        format!("{err:#}").contains("AGENTBOX_HOST_UID and AGENTBOX_HOST_GID are required"),
        "unexpected error: {err:#}"
    );
}

#[test]
fn identity_plan_keeps_root_for_interactive_fish_when_root_mode_is_requested() {
    let command = vec!["fish".to_owned(), "-l".to_owned()];
    let plan = derive_identity_plan(
        &command,
        ProcessIds { uid: 0, gid: 0 },
        &TestEnv::new(&[(ENTER_AS_ROOT_ENV, "1")]),
    )
    .expect("root mode should not require host identity when no drop is selected");

    assert!(!plan.drop_to_dev);
    assert_eq!(plan.identity.uid, 1000);
    assert_eq!(plan.identity.gid, 1000);
}

#[test]
fn identity_plan_root_mode_overrides_kvm_drop_for_final_exec_only() {
    let command = vec!["fish".to_owned(), "-l".to_owned()];
    let plan = derive_identity_plan(
        &command,
        ProcessIds { uid: 0, gid: 0 },
        &TestEnv::new(&[(ENTER_AS_ROOT_ENV, "1"), ("AGENTBOX_KVM_DROP_TO_DEV", "1")]),
    )
    .expect("root mode should override final drop decision");

    assert!(!plan.drop_to_dev);
}

#[test]
fn identity_plan_ignores_malformed_root_mode_env() {
    let command = vec!["fish".to_owned(), "-l".to_owned()];
    let err = derive_identity_plan(
        &command,
        ProcessIds { uid: 0, gid: 0 },
        &TestEnv::new(&[(ENTER_AS_ROOT_ENV, "true")]),
    )
    .expect_err("non-1 root mode value should not suppress required host identity");

    assert!(
        format!("{err:#}").contains("AGENTBOX_HOST_UID and AGENTBOX_HOST_GID are required"),
        "unexpected error: {err:#}"
    );
}

#[test]
fn identity_plan_keeps_non_root_runtime_identity() {
    let command = vec!["bash".to_owned(), "-lc".to_owned(), "id".to_owned()];
    let plan = derive_identity_plan(
        &command,
        ProcessIds {
            uid: 2000,
            gid: 2001,
        },
        &TestEnv::new(&[]),
    )
    .expect("non-root command should derive identity");

    assert!(!plan.drop_to_dev);
    assert_eq!(plan.identity.uid, 2000);
    assert_eq!(plan.identity.gid, 2001);
}

#[test]
fn normal_shell_environment_matches_container_entrypoint_contract() {
    let command = vec!["bash".to_owned(), "-lc".to_owned(), "true".to_owned()];
    let plan = derive_identity_plan(
        &command,
        ProcessIds {
            uid: 1000,
            gid: 1000,
        },
        &TestEnv::new(&[]),
    )
    .expect("identity should derive");

    let vars = normal_shell_environment(&plan.identity);

    for required in [
        ("USER", "dev"),
        ("HOME", "/home/dev"),
        ("XDG_CONFIG_HOME", "/home/dev/.config"),
        ("XDG_DATA_HOME", "/home/dev/.local/share"),
        ("XDG_STATE_HOME", "/home/dev/.local/state"),
        ("XDG_CACHE_HOME", "/home/dev/.cache"),
        ("TMPDIR", "/home/dev/.cache/tmp"),
    ] {
        assert!(
            vars.contains(&(required.0.to_owned(), required.1.to_owned())),
            "missing {required:?} in {vars:?}"
        );
    }
    assert!(
        vars.iter()
            .any(|(key, value)| key == "SHELL" && value.ends_with("fish")),
        "SHELL should remain the login fish shell even for explicit commands: {vars:?}"
    );
}

#[test]
fn nss_wrapper_plan_generates_dev_identity_and_preserves_old_preload() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let passwd_path = temp.path().join("passwd");
    let group_path = temp.path().join("group");
    fs::write(
        &passwd_path,
        "root:x:0:0:root:/root:/bin/sh\ndev:x:999:999:old:/old:/bin/sh\n",
    )
    .expect("passwd should be seeded");
    fs::write(&group_path, "root:x:0:\ndev:x:999:\n").expect("group should be seeded");
    let command = vec!["/nix/store/fish/bin/fish".to_owned(), "-l".to_owned()];
    let identity = derive_identity_plan(
        &command,
        ProcessIds { uid: 0, gid: 0 },
        &TestEnv::new(&[(HOST_UID_ENV, "1001"), (HOST_GID_ENV, "1002")]),
    )
    .expect("identity should derive")
    .identity;

    let plan = build_nss_wrapper_plan(
        &passwd_path,
        &group_path,
        &identity,
        "/old/libpreload.so",
        Some("/nix/store/nss/lib/libnss_wrapper.so"),
    )
    .expect("nss plan should build");

    assert_eq!(
        plan.passwd,
        format!(
            "root:x:0:0:root:/root:/bin/sh\ndev:x:1001:1002:dev user:/home/dev:{}\n",
            identity.shell.display()
        )
    );
    assert!(identity.shell.ends_with("fish"));
    assert_eq!(plan.group, "root:x:0:\ndev:x:1002:\n");
    assert_eq!(
        plan.ld_preload,
        "/nix/store/nss/lib/libnss_wrapper.so:/old/libpreload.so"
    );
}

#[test]
fn nss_wrapper_plan_requires_explicit_image_nss_library() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let identity = crate::guest_init::components::home::identity::DevIdentity::new(
        1000,
        1000,
        PathBuf::from("fish"),
    );
    let err = build_nss_wrapper_plan(
        &temp.path().join("passwd"),
        &temp.path().join("group"),
        &identity,
        "",
        None,
    )
    .expect_err("nss wrapper library path should be required");

    assert!(
        format!("{err:#}").contains("AGENTBOX_NSS_WRAPPER_LIB is required"),
        "unexpected error: {err:#}"
    );
}

#[test]
fn writable_dir_shadowing_follows_symlink_contents_like_cp_rl() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let source_target = temp.path().join("source-target");
    let source_link = temp.path().join("source-link");
    let shadow = temp.path().join("shadow");
    fs::create_dir_all(&source_target).expect("source target should be created");
    fs::write(source_target.join("config.toml"), "bundled config\n")
        .expect("source file should be written");
    symlink(&source_target, &source_link).expect("directory symlink should be created");

    materialize_writable_dir(&source_link, &shadow)
        .expect("symlinked writable dir should materialize");

    assert!(
        source_link.is_dir(),
        "source path should become a real directory"
    );
    assert!(
        !fs::symlink_metadata(&source_link)
            .expect("source metadata should be readable")
            .file_type()
            .is_symlink(),
        "source path should not remain a symlink"
    );
    assert_eq!(
        fs::read_to_string(source_link.join("config.toml"))
            .expect("copied config should be readable"),
        "bundled config\n"
    );
}

#[test]
fn container_enter_operation_order_keeps_normal_setup_before_exec() {
    let ops = planned_enter_operations();
    let pos = |op| ops.iter().position(|candidate| candidate == &op).unwrap();

    assert!(
        pos(ContainerEnterOperation::ResolveCommand) < pos(ContainerEnterOperation::DeriveIdentity)
    );
    assert!(
        pos(ContainerEnterOperation::ExportShellEnvironment)
            < pos(ContainerEnterOperation::MaterializeNssWrapper)
    );
    assert!(
        pos(ContainerEnterOperation::MaterializeHomeConfig)
            < pos(ContainerEnterOperation::DropAndExec)
    );
    assert!(
        pos(ContainerEnterOperation::ClearProfileEnvBeforeExec)
            < pos(ContainerEnterOperation::DropAndExec)
    );
    assert!(
        pos(ContainerEnterOperation::ReportProfileBeforeExec)
            < pos(ContainerEnterOperation::DropAndExec)
    );
}
