use super::task::{build_podman_args, TaskPodmanSpec};
use crate::*;
use std::path::PathBuf;

#[test]
fn build_podman_args_includes_persistent_nix_mounts() {
    let root = PersistentNixRoot::new(std::path::Path::new("/tmp/state/agentbox/project"));
    let runtime = NixRuntime::Seeded(root);
    let args = build_args(&runtime, TaskContainerMode::Native, false, None);
    assert_eq!(args[3], "--userns");
    assert_eq!(args[4], "keep-id");
    assert!(args.contains(&"--hostname".to_owned()));
    assert!(args.contains(&"project-agentbox".to_owned()));
    assert!(args.contains(&"/tmp/state/agentbox/project/nix/store:/nix/store".to_owned()));
    assert!(args.contains(&"/tmp/state/agentbox/project/nix/var/nix:/nix/var/nix".to_owned()));
    assert!(
        args.contains(&"/tmp/state/agentbox/project/nix/var/log/nix:/nix/var/log/nix".to_owned())
    );
    assert!(args.contains(&"/home/alice/.codex:/home/dev/.codex".to_owned()));
    assert!(args.contains(&"/tmp/state/agentbox/project/cargo:/home/dev/.cargo".to_owned()));
    assert!(args.contains(&"/tmp/state/agentbox/sccache:/home/dev/.cache/sccache".to_owned()));
    assert!(args.contains(&"--tmpfs".to_owned()));
    assert!(args.contains(&CONTAINER_TMP_TMPFS.to_owned()));
    assert!(args.contains(&"--env".to_owned()));
    assert!(args.contains(&format!("SCCACHE_DIR={CONTAINER_SCCACHE_DIR}")));
    assert_eq!(args[args.len() - 2], INTERACTIVE_SHELL);
    assert_eq!(args[args.len() - 1], "-l");
    assert!(!args.contains(&"--user".to_owned()));
    assert!(!args.contains(&format!("NIX_REMOTE={NIX_REMOTE_SOCKET}")));
    assert!(!args.contains(&TASK_KVM_DROP_TO_DEV_ENV.to_owned()));
    assert!(!args.contains(&"--runtime".to_owned()));
    assert!(!args.contains(&"run.oci.handler=krun".to_owned()));
    assert!(!args.contains(&KRUN_USE_PASST_ANNOTATION.to_owned()));
    assert!(!args
        .iter()
        .any(|arg| arg.starts_with(KRUN_RAM_MIB_ANNOTATION_PREFIX)));
    assert!(!args
        .iter()
        .any(|arg| arg.starts_with(KRUN_CPUS_ANNOTATION_PREFIX)));
    assert!(!args.contains(&NIX_NETWORK_DETECTION_PROXY_ENV.to_owned()));
    assert!(!args.iter().any(|a| a.starts_with(HOST_UID_ENV_PREFIX)));
    assert!(!args.iter().any(|a| a.starts_with(HOST_GID_ENV_PREFIX)));
}

#[test]
fn build_podman_args_includes_sidecar_nix_mount_and_remote() {
    let runtime = sidecar_runtime();
    let args = build_args(&runtime, TaskContainerMode::Native, false, None);

    assert!(args.contains(&"/tmp/state/agentbox/project/nix-merged:/nix:ro".to_owned()));
    assert!(args.contains(&"/tmp/state/agentbox/sccache:/home/dev/.cache/sccache".to_owned()));
    assert!(args.contains(&"--hostname".to_owned()));
    assert!(args.contains(&"project-agentbox".to_owned()));
    assert!(args.contains(&"--env".to_owned()));
    assert!(args.contains(&format!("SCCACHE_DIR={CONTAINER_SCCACHE_DIR}")));
    assert!(args.contains(&format!("NIX_REMOTE={NIX_REMOTE_SOCKET}")));
    assert!(args.contains(&"--label".to_owned()));
    assert!(args.contains(&format!(
        "{TASK_CONTAINER_ROLE_LABEL}={TASK_CONTAINER_ROLE_VALUE}"
    )));
    assert!(args.contains(&format!(
        "{TASK_CONTAINER_SIDECAR_LABEL}=agentbox-nix-sidecar-abc"
    )));
    assert!(!args.contains(&"/tmp/state/agentbox/project/nix/store:/nix/store".to_owned()));
    assert!(!args.contains(&"/tmp/state/agentbox/project/nix/var/nix:/nix/var/nix".to_owned()));
    assert!(!args.contains(&TASK_KVM_DROP_TO_DEV_ENV.to_owned()));
    assert!(!args.contains(&"--runtime".to_owned()));
    assert!(!args.contains(&"run.oci.handler=krun".to_owned()));
    assert!(!args.contains(&KRUN_USE_PASST_ANNOTATION.to_owned()));
    assert!(!args
        .iter()
        .any(|arg| arg.starts_with(KRUN_RAM_MIB_ANNOTATION_PREFIX)));
    assert!(!args
        .iter()
        .any(|arg| arg.starts_with(KRUN_CPUS_ANNOTATION_PREFIX)));
    assert!(!args.contains(&NIX_NETWORK_DETECTION_PROXY_ENV.to_owned()));
    assert!(!args.iter().any(|a| a.starts_with(HOST_UID_ENV_PREFIX)));
    assert!(!args.iter().any(|a| a.starts_with(HOST_GID_ENV_PREFIX)));
    assert_eq!(args[args.len() - 2], INTERACTIVE_SHELL);
    assert_eq!(args[args.len() - 1], "-l");
}

#[test]
fn build_podman_args_adds_only_libkrun_runtime_args_for_libkrun_task_mode() {
    let runtime = sidecar_runtime();
    let native_args = build_args(&runtime, TaskContainerMode::Native, false, None);
    let libkrun_args = build_args(&runtime, TaskContainerMode::Libkrun, false, Some(12345));

    // Verify host UID/GID env vars are present in libkrun mode.
    let has_host_uid = libkrun_args
        .windows(2)
        .any(|w| w[0] == "--env" && w[1].starts_with(HOST_UID_ENV_PREFIX));
    assert!(
        has_host_uid,
        "libkrun args should include AGENTBOX_HOST_UID env var"
    );
    let has_host_gid = libkrun_args
        .windows(2)
        .any(|w| w[0] == "--env" && w[1].starts_with(HOST_GID_ENV_PREFIX));
    assert!(
        has_host_gid,
        "libkrun args should include AGENTBOX_HOST_GID env var"
    );
    assert!(
        libkrun_args
            .windows(2)
            .any(|w| w[0] == "--annotation" && w[1] == "run.oci.handler=krun"),
        "libkrun args should include run.oci.handler=krun annotation"
    );
    assert!(
        has_arg_pair(&libkrun_args, "--annotation", "krun.ram_mib=8192"),
        "libkrun args should include krun.ram_mib annotation"
    );
    assert!(
        !libkrun_args
            .iter()
            .any(|arg| arg.starts_with(KRUN_CPUS_ANNOTATION_PREFIX)),
        "libkrun args should omit krun.cpus when no CPU count is resolved"
    );
    assert!(
        has_arg_pair(&libkrun_args, "--annotation", KRUN_USE_PASST_ANNOTATION),
        "libkrun args should include krun.use_passt=1 by default"
    );
    assert!(
        !has_arg_pair(&libkrun_args, "--env", NIX_NETWORK_DETECTION_PROXY_ENV),
        "libkrun args with default passt should omit proxy workaround"
    );

    let mut libkrun_without_libkrun_args = libkrun_args;
    remove_arg_pair(
        &mut libkrun_without_libkrun_args,
        "--env",
        TASK_KVM_DROP_TO_DEV_ENV,
    );
    remove_arg_pair_with(&mut libkrun_without_libkrun_args, "--env", |arg| {
        arg.starts_with(HOST_UID_ENV_PREFIX)
    });
    remove_arg_pair_with(&mut libkrun_without_libkrun_args, "--env", |arg| {
        arg.starts_with(HOST_GID_ENV_PREFIX)
    });
    remove_arg_pair(&mut libkrun_without_libkrun_args, "--runtime", "crun");
    remove_arg_pair(
        &mut libkrun_without_libkrun_args,
        "--annotation",
        "run.oci.handler=krun",
    );
    remove_arg_pair(
        &mut libkrun_without_libkrun_args,
        "--annotation",
        "krun.ram_mib=8192",
    );
    remove_arg_pair(
        &mut libkrun_without_libkrun_args,
        "--annotation",
        KRUN_USE_PASST_ANNOTATION,
    );

    // Libkrun mode uses a guest-local NIX_REMOTE, passes the proxy port env var,
    // and passes the proxy host env var. Replace these with the native
    // equivalents or strip them before comparing.
    remove_arg_pair(
        &mut libkrun_without_libkrun_args,
        "--env",
        &format!("{KVM_NIX_PROXY_PORT_ENV}=12345"),
    );
    remove_arg_pair_with(&mut libkrun_without_libkrun_args, "--env", |arg| {
        arg.starts_with(KVM_NIX_PROXY_HOST_ENV)
    });
    if let Some(pos) = libkrun_without_libkrun_args.windows(2).position(|w| {
        w[0] == "--env" && w[1] == format!("NIX_REMOTE={KVM_NIX_PROXY_GUEST_NIX_REMOTE}")
    }) {
        libkrun_without_libkrun_args[pos + 1] = format!("NIX_REMOTE={NIX_REMOTE_SOCKET}");
    }

    assert_eq!(libkrun_without_libkrun_args, native_args);
}

#[test]
fn build_podman_args_includes_libkrun_cpu_annotation_when_resolved() {
    let runtime = sidecar_runtime();
    let args = build_args_with_cpu(
        &runtime,
        TaskContainerMode::Libkrun,
        false,
        Some(12345),
        Some(16),
    );

    assert!(
        has_arg_pair(&args, "--annotation", "run.oci.handler=krun"),
        "libkrun args should include run.oci.handler=krun annotation"
    );
    assert!(
        has_arg_pair(&args, "--annotation", "krun.ram_mib=8192"),
        "libkrun args should include krun.ram_mib annotation"
    );
    assert!(
        has_arg_pair(&args, "--annotation", "krun.cpus=16"),
        "libkrun args should include krun.cpus annotation when CPU count is resolved"
    );
}

#[test]
fn build_podman_args_omits_libkrun_cpu_annotation_when_unresolved() {
    let runtime = sidecar_runtime();
    let args = build_args_with_cpu(
        &runtime,
        TaskContainerMode::Libkrun,
        false,
        Some(12345),
        None,
    );

    assert!(
        !args
            .iter()
            .any(|arg| arg.starts_with(KRUN_CPUS_ANNOTATION_PREFIX)),
        "libkrun args should omit krun.cpus when no CPU count is resolved"
    );
}

#[test]
fn build_podman_args_treats_libkrun_cpu_count_as_noop_for_native_task_mode() {
    let runtime = sidecar_runtime();
    let args = build_args_with_cpu(&runtime, TaskContainerMode::Native, false, None, Some(16));

    assert!(
        !args
            .iter()
            .any(|arg| arg.starts_with(KRUN_CPUS_ANNOTATION_PREFIX)),
        "native args should not include krun.cpus"
    );
}

#[test]
fn build_podman_args_enables_passt_by_default_for_libkrun_task_mode() {
    let runtime = sidecar_runtime();
    let args = build_args(&runtime, TaskContainerMode::Libkrun, false, Some(12345));

    assert!(
        has_arg_pair(&args, "--annotation", KRUN_USE_PASST_ANNOTATION),
        "libkrun args should include krun.use_passt=1 by default"
    );
    assert!(
        !has_arg_pair(&args, "--env", NIX_NETWORK_DETECTION_PROXY_ENV),
        "libkrun args should not include the proxy workaround with default passt"
    );
}

#[test]
fn build_podman_args_uses_tsi_when_requested_for_libkrun_task_mode() {
    let runtime = sidecar_runtime();
    let args = build_args(&runtime, TaskContainerMode::Libkrun, true, Some(12345));

    assert!(
        !has_arg_pair(&args, "--annotation", KRUN_USE_PASST_ANNOTATION),
        "libkrun TSI args should not include krun.use_passt=1"
    );
    assert!(
        has_arg_pair(&args, "--env", NIX_NETWORK_DETECTION_PROXY_ENV),
        "libkrun TSI args should include the proxy workaround"
    );
}

#[test]
fn build_podman_args_treats_tsi_as_noop_for_native_task_mode() {
    let runtime = sidecar_runtime();
    let args = build_args(&runtime, TaskContainerMode::Native, true, None);

    assert!(
        !has_arg_pair(&args, "--annotation", KRUN_USE_PASST_ANNOTATION),
        "native args should not include krun.use_passt=1"
    );
    assert!(
        !has_arg_pair(&args, "--env", NIX_NETWORK_DETECTION_PROXY_ENV),
        "native args should not include the proxy workaround"
    );
}

#[test]
fn build_podman_args_rejects_libkrun_mode_without_resolved_memory() {
    let runtime = sidecar_runtime();
    let err = build_args_result(&runtime, TaskContainerMode::Libkrun, false, None, None)
        .expect_err("libkrun without resolved memory should fail");

    assert!(err.to_string().contains("requires a resolved krun.ram_mib"));
}

fn sidecar_runtime() -> NixRuntime {
    NixRuntime::Sidecar(SidecarNixRuntime {
        merged_dir: PathBuf::from("/tmp/state/agentbox/project/nix-merged"),
        sidecar_name: "agentbox-nix-sidecar-abc".to_owned(),
        proxy_port: 19876,
    })
}

fn build_args(
    nix_runtime: &NixRuntime,
    task_mode: TaskContainerMode,
    use_tsi: bool,
    proxy_port: Option<u16>,
) -> Vec<String> {
    let libkrun_ram_mib = if task_mode == TaskContainerMode::Libkrun {
        Some(8192)
    } else {
        None
    };
    build_args_with_mem(nix_runtime, task_mode, use_tsi, proxy_port, libkrun_ram_mib)
}

fn build_args_with_mem(
    nix_runtime: &NixRuntime,
    task_mode: TaskContainerMode,
    use_tsi: bool,
    proxy_port: Option<u16>,
    libkrun_ram_mib: Option<u32>,
) -> Vec<String> {
    build_args_result(nix_runtime, task_mode, use_tsi, proxy_port, libkrun_ram_mib)
        .expect("podman args should build")
}

fn build_args_result(
    nix_runtime: &NixRuntime,
    task_mode: TaskContainerMode,
    use_tsi: bool,
    proxy_port: Option<u16>,
    libkrun_ram_mib: Option<u32>,
) -> anyhow::Result<Vec<String>> {
    build_args_result_with_cpu(
        nix_runtime,
        task_mode,
        use_tsi,
        proxy_port,
        libkrun_ram_mib,
        None,
    )
}

fn build_args_with_cpu(
    nix_runtime: &NixRuntime,
    task_mode: TaskContainerMode,
    use_tsi: bool,
    proxy_port: Option<u16>,
    libkrun_cpu_count: Option<u32>,
) -> Vec<String> {
    let libkrun_ram_mib = if task_mode == TaskContainerMode::Libkrun {
        Some(8192)
    } else {
        None
    };
    build_args_result_with_cpu(
        nix_runtime,
        task_mode,
        use_tsi,
        proxy_port,
        libkrun_ram_mib,
        libkrun_cpu_count,
    )
    .expect("podman args should build")
}

fn build_args_result_with_cpu(
    nix_runtime: &NixRuntime,
    task_mode: TaskContainerMode,
    use_tsi: bool,
    proxy_port: Option<u16>,
    libkrun_ram_mib: Option<u32>,
    libkrun_cpu_count: Option<u32>,
) -> anyhow::Result<Vec<String>> {
    build_podman_args(TaskPodmanSpec {
        image: DEFAULT_IMAGE,
        hostname: "project-agentbox",
        workspace_mount: "/tmp/project:/workspace",
        codex_mount: "/home/alice/.codex:/home/dev/.codex",
        cargo_mount: "/tmp/state/agentbox/project/cargo:/home/dev/.cargo",
        sccache_mount: "/tmp/state/agentbox/sccache:/home/dev/.cache/sccache",
        nix_runtime,
        task_mode,
        use_tsi,
        libkrun_ram_mib,
        libkrun_cpu_count,
        proxy_port,
    })
}

fn has_arg_pair(args: &[String], flag: &str, value: &str) -> bool {
    args.windows(2).any(|w| w[0] == flag && w[1] == value)
}

fn remove_arg_pair(args: &mut Vec<String>, flag: &str, value: &str) {
    remove_arg_pair_with(args, flag, |arg| arg == value);
}

fn remove_arg_pair_with(args: &mut Vec<String>, flag: &str, value_matches: impl Fn(&str) -> bool) {
    while let Some(pos) = args
        .windows(2)
        .position(|w| w[0] == flag && value_matches(&w[1]))
    {
        args.drain(pos..pos + 2);
    }
}
