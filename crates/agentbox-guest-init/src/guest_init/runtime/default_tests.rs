use std::collections::BTreeMap;

use super::{
    container_dispatch_argv_for_exe, libkrun_dispatch_argv_for_exe, planned_enter_operations,
    should_dispatch_libkrun, DefaultEnterOperation, LIBKRUN_CONTAINERS_STORAGE_ENV,
    LIBKRUN_NIX_OVERLAY_ENV,
};

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
fn default_enter_operation_order_dispatches_before_container_fallback() {
    let ops = planned_enter_operations();
    let pos = |op| ops.iter().position(|candidate| candidate == &op).unwrap();

    assert!(
        pos(DefaultEnterOperation::ResolveCommand)
            < pos(DefaultEnterOperation::DispatchLibkrunIfRequested)
    );
    assert!(
        pos(DefaultEnterOperation::DispatchLibkrunIfRequested)
            < pos(DefaultEnterOperation::DispatchContainer)
    );
}

#[test]
fn default_libkrun_dispatch_preserves_resolved_command_after_separator() {
    let argv = libkrun_dispatch_argv_for_exe(
        "/nix/store/guest/bin/agentbox-guest-init",
        &["fish".to_owned(), "-l".to_owned()],
    );

    assert_eq!(
        argv,
        [
            "/nix/store/guest/bin/agentbox-guest-init",
            "libkrun",
            "enter",
            "--",
            "fish",
            "-l"
        ]
    );
}

#[test]
fn default_container_dispatch_preserves_resolved_command_after_separator() {
    let argv = container_dispatch_argv_for_exe(
        "/nix/store/guest/bin/agentbox-guest-init",
        &["bash".to_owned(), "-lc".to_owned(), "true".to_owned()],
    );

    assert_eq!(
        argv,
        [
            "/nix/store/guest/bin/agentbox-guest-init",
            "container",
            "enter",
            "--",
            "bash",
            "-lc",
            "true"
        ]
    );
}

#[test]
fn default_libkrun_dispatch_is_enabled_by_nix_overlay_flag() {
    assert!(should_dispatch_libkrun(&TestEnv::new(&[(
        LIBKRUN_NIX_OVERLAY_ENV,
        "1",
    )])));
}

#[test]
fn default_libkrun_dispatch_is_enabled_by_containers_storage_flag() {
    assert!(should_dispatch_libkrun(&TestEnv::new(&[(
        LIBKRUN_CONTAINERS_STORAGE_ENV,
        "1",
    )])));
}

#[test]
fn default_libkrun_dispatch_ignores_unset_or_non_one_flags() {
    assert!(!should_dispatch_libkrun(&TestEnv::new(&[])));
    assert!(!should_dispatch_libkrun(&TestEnv::new(&[(
        LIBKRUN_NIX_OVERLAY_ENV,
        "0",
    )])));
}
