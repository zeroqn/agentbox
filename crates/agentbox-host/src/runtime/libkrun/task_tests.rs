use crate::runtime::components::diagnostics::{
    GUEST_DEBUG_ENV, GUEST_DIAGNOSTICS_OWNER, GUEST_PROFILE_ENV,
};
use crate::runtime::components::identity::USER_IDENTITY_OWNER;
use crate::runtime::components::volumes::{
    TaskVolumeMounts, SCCACHE_VOLUME_OWNER, WORKSPACE_VOLUME_OWNER,
};
use crate::runtime::libkrun::components::cpu::{CPU_OWNER, LIBKRUN_CPUS_ANNOTATION_PREFIX};
use crate::runtime::libkrun::components::debug::DEBUG_OWNER;
use crate::runtime::libkrun::components::debug::{DebugEntrypointMount, DebugGuestInitMount};
use crate::runtime::libkrun::components::disk::containers::podman::{
    CONTAINERS_DISK_OWNER, LIBKRUN_CONTAINERS_STORAGE_ENV,
};
use crate::runtime::libkrun::components::disk::containers::raw_image::{
    RawContainerDisk, RawContainerDiskStatus, RAW_CONTAINER_DISK_LABEL,
    RAW_CONTAINER_DISK_SIZE_BYTES,
};
use crate::runtime::libkrun::components::disk::nix::podman::{
    LIBKRUN_NIX_OVERLAY_ENV, NIX_DISK_OWNER,
};
use crate::runtime::libkrun::components::disk::nix::raw_image::{
    RawNixDisk, RawNixDiskStatus, RAW_NIX_DISK_LABEL, RAW_NIX_DISK_SIZE_BYTES,
};
use crate::runtime::libkrun::components::host_identity::{
    HOST_IDENTITY_OWNER, LIBKRUN_KVM_DROP_TO_DEV_ENV,
};
use crate::runtime::libkrun::components::memory::MEMORY_OWNER;
use crate::runtime::libkrun::components::network;
use crate::runtime::libkrun::components::oci::{LIBKRUN_HANDLER_ANNOTATION, OCI_OWNER};
use crate::runtime::libkrun::task::{build_libkrun_task_podman_args, build_libkrun_task_run_args};
use crate::{CONTAINER_SCCACHE_DIR, CONTAINER_TMP_TMPFS, INTERACTIVE_SHELL};
use std::path::PathBuf;

#[test]
fn libkrun_task_args_match_ordered_default_passt_baseline() {
    let disk = raw_disk();
    let container_disk = raw_container_disk();
    let args = build_args(&disk, &container_disk);

    assert_eq!(args, expected_args(ExpectedOptions::default()));
}

#[test]
fn libkrun_task_args_match_ordered_tsi_baseline() {
    let disk = raw_disk();
    let container_disk = raw_container_disk();
    let args = build_args_with_tsi(&disk, &container_disk, true);

    assert_eq!(
        args,
        expected_args(ExpectedOptions {
            tsi: true,
            ..Default::default()
        })
    );
}

#[test]
fn libkrun_task_args_match_ordered_cpu_absent_baseline() {
    let disk = raw_disk();
    let container_disk = raw_container_disk();
    let args = build_args_with_cpu_count(&disk, &container_disk, None);

    assert_eq!(
        args,
        expected_args(ExpectedOptions {
            include_cpu: false,
            ..Default::default()
        })
    );
}

#[test]
fn libkrun_task_args_match_ordered_debug_entrypoint_baseline() {
    let disk = raw_disk();
    let container_disk = raw_container_disk();
    let debug_entrypoint = debug_entrypoint();
    let args = build_args_with_debug_entrypoint(&disk, &container_disk, Some(&debug_entrypoint));

    assert_eq!(
        args,
        expected_args(ExpectedOptions {
            debug_entrypoint: Some(debug_entrypoint.mount_arg.clone()),
            ..Default::default()
        })
    );
}

#[test]
fn libkrun_task_args_match_ordered_debug_guest_init_baseline() {
    let disk = raw_disk();
    let container_disk = raw_container_disk();
    let debug_guest_init = debug_guest_init();
    let args = build_args_with_debug_guest_init(&disk, &container_disk, Some(&debug_guest_init));

    assert_eq!(
        args,
        expected_args(ExpectedOptions {
            debug_guest_init: Some(debug_guest_init.mount_arg.clone()),
            ..Default::default()
        })
    );
}

#[test]
fn libkrun_task_args_include_krun_disk_annotations_and_guest_overlay_env() {
    let disk = raw_disk();
    let container_disk = raw_container_disk();
    let args = build_args(&disk, &container_disk);
    let joined = args.join("\n");

    assert_eq!(args[0], "run");
    assert!(args.contains(&"--name".to_owned()));
    assert!(args.contains(&"project-random".to_owned()));
    assert!(args.contains(&"--runtime".to_owned()));
    assert!(args.contains(&"crun".to_owned()));
    assert!(args.contains(&LIBKRUN_HANDLER_ANNOTATION.to_owned()));
    assert!(args.contains(&"krun.ram_mib=8192".to_owned()));
    assert!(args.contains(&"krun.cpus=16".to_owned()));
    assert!(
        args.contains(&"krun.disk.0.path=/tmp/state/agentbox/project/libkrun-nix.raw".to_owned())
    );
    assert!(args.contains(&"krun.disk.0.id=agentbox-nix".to_owned()));
    assert!(args.contains(&"krun.disk.0.readonly=false".to_owned()));
    assert!(args.contains(
        &"krun.disk.1.path=/tmp/state/agentbox/project/libkrun-containers.raw".to_owned()
    ));
    assert!(args.contains(&"krun.disk.1.id=agentbox-containers".to_owned()));
    assert!(args.contains(&"krun.disk.1.readonly=false".to_owned()));
    assert!(joined.contains(&format!("--device\n{}", network::LIBKRUN_TUN_DEVICE)));
    assert!(args.contains(&LIBKRUN_NIX_OVERLAY_ENV.to_owned()));
    assert!(args.contains(&"AGENTBOX_LIBKRUN_NIX_DISK_ID=agentbox-nix".to_owned()));
    assert!(args.contains(&format!(
        "AGENTBOX_LIBKRUN_NIX_DISK_LABEL={RAW_NIX_DISK_LABEL}"
    )));
    assert!(args.contains(&LIBKRUN_CONTAINERS_STORAGE_ENV.to_owned()));
    assert!(args.contains(&"AGENTBOX_LIBKRUN_CONTAINERS_DISK_ID=agentbox-containers".to_owned()));
    assert!(args.contains(&format!(
        "AGENTBOX_LIBKRUN_CONTAINERS_DISK_LABEL={RAW_CONTAINER_DISK_LABEL}"
    )));
    assert!(args.contains(&"AGENTBOX_HOST_UID=1001".to_owned()));
    assert!(args.contains(&"AGENTBOX_HOST_GID=1002".to_owned()));
    assert!(args.contains(&LIBKRUN_KVM_DROP_TO_DEV_ENV.to_owned()));
    assert!(joined.contains("--userns\nkeep-id"));
    assert!(!joined.contains("keep-id:"));
    assert!(joined.contains("--user\n0:0"));
    assert!(args.contains(&"/tmp/project:/workspace".to_owned()));
    assert!(args.contains(&"/home/alice/.codex:/home/dev/.codex".to_owned()));
    assert!(args.contains(&"/tmp/state/agentbox/project/cargo:/home/dev/.cargo".to_owned()));
    assert!(args.contains(&"/tmp/state/agentbox/sccache:/home/dev/.cache/sccache".to_owned()));
    assert!(args.contains(&format!("SCCACHE_DIR={CONTAINER_SCCACHE_DIR}")));
    assert!(args.contains(&CONTAINER_TMP_TMPFS.to_owned()));
    assert!(args.contains(&network::LIBKRUN_USE_PASST_ENV.to_owned()));
    assert!(args.contains(&network::LIBKRUN_USE_PASST_ANNOTATION.to_owned()));
    assert!(!joined.contains("--memory"));
    assert!(!args.contains(&network::LIBKRUN_TSI_PROXY_ENV.to_owned()));
    assert_eq!(args[args.len() - 2], INTERACTIVE_SHELL);
    assert_eq!(args[args.len() - 1], "-l");
}

#[test]
fn libkrun_task_args_expose_component_owner_ownership() {
    let disk = raw_disk();
    let container_disk = raw_container_disk();
    let args = build_run_args(&disk, &container_disk);

    assert!(args.contains_option_from(USER_IDENTITY_OWNER, "--userns", "keep-id"));
    assert!(args.contains_option_from(OCI_OWNER, "--runtime", "crun"));
    assert!(args.contains_option_from(OCI_OWNER, "--annotation", LIBKRUN_HANDLER_ANNOTATION));
    assert!(args.contains_option_from(MEMORY_OWNER, "--annotation", "krun.ram_mib=8192"));
    assert!(args.contains_option_from(CPU_OWNER, "--annotation", "krun.cpus=16"));
    assert!(args.contains_option_from(
        NIX_DISK_OWNER,
        "--annotation",
        "krun.disk.0.id=agentbox-nix"
    ));
    assert!(args.contains_option_from(
        NIX_DISK_OWNER,
        "--env",
        "AGENTBOX_LIBKRUN_NIX_DISK_ID=agentbox-nix"
    ));
    assert!(args.contains_option_from(
        CONTAINERS_DISK_OWNER,
        "--annotation",
        "krun.disk.1.id=agentbox-containers"
    ));
    assert!(args.contains_option_from(
        CONTAINERS_DISK_OWNER,
        "--env",
        "AGENTBOX_LIBKRUN_CONTAINERS_DISK_ID=agentbox-containers"
    ));
    assert!(args.contains_option_from(
        network::NETWORK_OWNER,
        "--device",
        network::LIBKRUN_TUN_DEVICE
    ));
    assert!(args.contains_option_from(
        network::NETWORK_OWNER,
        "--env",
        network::LIBKRUN_USE_PASST_ENV
    ));
    assert!(args.contains_option_from(
        network::NETWORK_OWNER,
        "--annotation",
        network::LIBKRUN_USE_PASST_ANNOTATION
    ));
    assert!(args.contains_option_from(HOST_IDENTITY_OWNER, "--user", "0:0"));
    assert!(args.contains_option_from(HOST_IDENTITY_OWNER, "--env", "AGENTBOX_HOST_UID=1001"));
    assert!(args.contains_option_from(
        WORKSPACE_VOLUME_OWNER,
        "--volume",
        "/tmp/project:/workspace"
    ));
    assert!(args.contains_option_from(
        SCCACHE_VOLUME_OWNER,
        "--env",
        &format!("SCCACHE_DIR={CONTAINER_SCCACHE_DIR}")
    ));
}

#[test]
fn libkrun_debug_args_are_owned_by_debug_owner() {
    let disk = raw_disk();
    let container_disk = raw_container_disk();
    let debug_entrypoint = debug_entrypoint();
    let debug_guest_init = debug_guest_init();
    let args = build_run_args_with_options(
        &disk,
        &container_disk,
        false,
        Some(16),
        false,
        false,
        Some(&debug_entrypoint),
        Some(&debug_guest_init),
    );

    assert!(args.contains_option_from(DEBUG_OWNER, "--volume", &debug_entrypoint.mount_arg));
    assert!(args.contains_option_from(DEBUG_OWNER, "--entrypoint", debug_entrypoint.target));
    assert!(args.contains_option_from(DEBUG_OWNER, "--volume", &debug_guest_init.mount_arg));
}

#[test]
fn libkrun_tsi_proxy_env_is_owned_by_network_owner() {
    let disk = raw_disk();
    let container_disk = raw_container_disk();
    let args = build_run_args_with_options(
        &disk,
        &container_disk,
        true,
        Some(16),
        false,
        false,
        None,
        None,
    );

    assert!(args.contains_option_from(
        network::NETWORK_OWNER,
        "--env",
        network::LIBKRUN_TSI_PROXY_ENV
    ));
    assert!(!args.contains_option_from(
        network::NETWORK_OWNER,
        "--env",
        network::LIBKRUN_USE_PASST_ENV
    ));
}

#[test]
fn libkrun_guest_diagnostics_are_owned_by_guest_diagnostics() {
    let disk = raw_disk();
    let container_disk = raw_container_disk();
    let args = build_run_args_with_options(
        &disk,
        &container_disk,
        false,
        Some(16),
        true,
        true,
        None,
        None,
    );

    assert!(args.contains_option_from(GUEST_DIAGNOSTICS_OWNER, "--env", GUEST_PROFILE_ENV));
    assert!(args.contains_option_from(GUEST_DIAGNOSTICS_OWNER, "--env", GUEST_DEBUG_ENV));
}

#[test]
fn libkrun_task_args_exclude_container_sidecar_and_nix_proxy_paths() {
    let disk = raw_disk();
    let container_disk = raw_container_disk();
    let args = build_args(&disk, &container_disk);
    let joined = args.join("\n");

    assert!(!joined.contains("io.agentbox.sidecar"));
    assert!(!joined.contains("agentbox-nix-sidecar"));
    assert!(!joined.contains("AGENTBOX_NIX_PROXY_HOST"));
    assert!(!joined.contains("AGENTBOX_NIX_PROXY_PORT"));
    assert!(!joined.contains("NIX_REMOTE=unix:///nix/var/nix/daemon-socket/socket"));
    assert!(!joined.contains("/nix-merged:/nix"));
    assert!(!joined.contains("/nix/store:/nix/store"));
    assert!(!joined.contains("/nix/var/nix:/nix/var/nix"));
}

#[test]
fn libkrun_task_args_use_tsi_proxy_env_instead_of_default_passt_when_requested() {
    let disk = raw_disk();
    let container_disk = raw_container_disk();
    let args = build_args_with_tsi(&disk, &container_disk, true);
    let joined = args.join("\n");

    assert!(joined.contains("--annotation\nkrun.ram_mib=8192"));
    assert!(joined.contains("--annotation\nkrun.cpus=16"));
    assert!(joined.contains("--env\nno_proxy=1"));
    assert!(!args.contains(&network::LIBKRUN_USE_PASST_ANNOTATION.to_owned()));
    assert!(!args.contains(&network::LIBKRUN_USE_PASST_ENV.to_owned()));
    assert!(!joined.contains("--memory"));
}

#[test]
fn libkrun_task_args_omit_cpu_annotation_when_cpu_count_is_unresolved() {
    let disk = raw_disk();
    let container_disk = raw_container_disk();
    let args = build_args_with_cpu_count(&disk, &container_disk, None);
    let joined = args.join("\n");

    assert!(joined.contains("--annotation\nkrun.ram_mib=8192"));
    assert!(!args
        .iter()
        .any(|arg| arg.starts_with(LIBKRUN_CPUS_ANNOTATION_PREFIX)));
    assert!(args.contains(&network::LIBKRUN_USE_PASST_ANNOTATION.to_owned()));
    assert!(!joined.contains("--memory"));
}

#[test]
fn libkrun_task_args_can_override_entrypoint_for_guest_debugging() {
    let disk = raw_disk();
    let container_disk = raw_container_disk();
    let debug_entrypoint = debug_entrypoint();
    let args = build_args_with_debug_entrypoint(&disk, &container_disk, Some(&debug_entrypoint));
    let joined = args.join("\n");

    assert!(joined.contains("--volume\n/tmp/debug-entrypoint.sh:/bin/agentbox-debug-entrypoint:ro"));
    assert!(joined.contains("--entrypoint\n/bin/agentbox-debug-entrypoint"));
    assert!(joined.contains(&format!(
        "--entrypoint\n/bin/agentbox-debug-entrypoint\n{}\n{}\n-l",
        crate::DEFAULT_IMAGE,
        INTERACTIVE_SHELL
    )));
}

#[test]
fn libkrun_task_args_can_override_guest_init_without_changing_entrypoint() {
    let disk = raw_disk();
    let container_disk = raw_container_disk();
    let debug_guest_init = debug_guest_init();
    let args = build_args_with_debug_guest_init(&disk, &container_disk, Some(&debug_guest_init));
    let joined = args.join("\n");

    assert!(joined.contains(
        "--volume\n/tmp/agentbox-guest-init:/nix/store/hash-agentbox/bin/agentbox-guest-init:ro"
    ));
    assert!(!args.contains(&"--entrypoint".to_owned()));
    assert!(joined.contains(&format!(
        "{}\n{}\n-l",
        crate::DEFAULT_IMAGE,
        INTERACTIVE_SHELL
    )));
}

#[test]
fn libkrun_task_args_include_guest_profile_and_debug_env_when_requested() {
    let disk = raw_disk();
    let container_disk = raw_container_disk();
    let args = build_args_with_guest_diagnostics(&disk, &container_disk, true, true);

    assert!(args.contains(&GUEST_PROFILE_ENV.to_owned()));
    assert!(args.contains(&GUEST_DEBUG_ENV.to_owned()));
}

#[test]
fn libkrun_task_args_omit_guest_profile_and_debug_env_by_default() {
    let disk = raw_disk();
    let container_disk = raw_container_disk();
    let args = build_args(&disk, &container_disk);

    assert!(!args.contains(&GUEST_PROFILE_ENV.to_owned()));
    assert!(!args.contains(&GUEST_DEBUG_ENV.to_owned()));
}

#[test]
fn libkrun_task_args_can_pass_guest_debug_without_profile() {
    let disk = raw_disk();
    let container_disk = raw_container_disk();
    let args = build_args_with_guest_diagnostics(&disk, &container_disk, false, true);

    assert!(!args.contains(&GUEST_PROFILE_ENV.to_owned()));
    assert!(args.contains(&GUEST_DEBUG_ENV.to_owned()));
}

struct ExpectedOptions {
    tsi: bool,
    include_cpu: bool,
    debug_entrypoint: Option<String>,
    debug_guest_init: Option<String>,
}

impl Default for ExpectedOptions {
    fn default() -> Self {
        Self {
            tsi: false,
            include_cpu: true,
            debug_entrypoint: None,
            debug_guest_init: None,
        }
    }
}

fn default_task_volumes() -> TaskVolumeMounts {
    TaskVolumeMounts {
        workspace: "/tmp/project:/workspace".to_owned(),
        codex: "/home/alice/.codex:/home/dev/.codex".to_owned(),
        cargo: "/tmp/state/agentbox/project/cargo:/home/dev/.cargo".to_owned(),
        sccache: "/tmp/state/agentbox/sccache:/home/dev/.cache/sccache".to_owned(),
    }
}

fn expected_args(options: ExpectedOptions) -> Vec<String> {
    let mut args = [
        "run",
        "--rm",
        "-it",
        "--name",
        "project-random",
        "--userns",
        "keep-id",
        "--user",
        "0:0",
        "--runtime",
        "crun",
        "--annotation",
        "run.oci.handler=krun",
        "--annotation",
        "krun.ram_mib=8192",
        "--annotation",
        "krun.disk.0.path=/tmp/state/agentbox/project/libkrun-nix.raw",
        "--annotation",
        "krun.disk.0.id=agentbox-nix",
        "--annotation",
        "krun.disk.0.readonly=false",
        "--annotation",
        "krun.disk.1.path=/tmp/state/agentbox/project/libkrun-containers.raw",
        "--annotation",
        "krun.disk.1.id=agentbox-containers",
        "--annotation",
        "krun.disk.1.readonly=false",
        "--device",
        "/dev/net/tun:/dev/net/tun",
        "--workdir",
        "/workspace",
        "--hostname",
        "project-agentbox",
        "--volume",
        "/tmp/project:/workspace",
        "--volume",
        "/home/alice/.codex:/home/dev/.codex",
        "--volume",
        "/tmp/state/agentbox/project/cargo:/home/dev/.cargo",
        "--volume",
        "/tmp/state/agentbox/sccache:/home/dev/.cache/sccache",
        "--env",
        "SCCACHE_DIR=/home/dev/.cache/sccache",
        "--env",
        "AGENTBOX_LIBKRUN_NIX_OVERLAY=1",
        "--env",
        "AGENTBOX_LIBKRUN_NIX_DISK_ID=agentbox-nix",
        "--env",
        "AGENTBOX_LIBKRUN_NIX_DISK_LABEL=AGENTBOX_NIX",
        "--env",
        "AGENTBOX_LIBKRUN_CONTAINERS_STORAGE=1",
        "--env",
        "AGENTBOX_LIBKRUN_CONTAINERS_DISK_ID=agentbox-containers",
        "--env",
        "AGENTBOX_LIBKRUN_CONTAINERS_DISK_LABEL=AGENTBOX_CONTAINERS",
        "--env",
        "AGENTBOX_HOST_UID=1001",
        "--env",
        "AGENTBOX_HOST_GID=1002",
        "--env",
        "AGENTBOX_KVM_DROP_TO_DEV=1",
        "--tmpfs",
        "/tmp:rw,exec,mode=1777",
    ]
    .map(str::to_owned)
    .to_vec();

    if options.include_cpu {
        args.extend(["--annotation".to_owned(), "krun.cpus=16".to_owned()]);
    }

    if options.tsi {
        args.extend(["--env".to_owned(), "no_proxy=1".to_owned()]);
    } else {
        args.extend([
            "--env".to_owned(),
            "AGENTBOX_LIBKRUN_USE_PASST=1".to_owned(),
            "--annotation".to_owned(),
            "krun.use_passt=1".to_owned(),
        ]);
    }

    if let Some(mount_arg) = options.debug_entrypoint {
        args.extend([
            "--volume".to_owned(),
            mount_arg,
            "--entrypoint".to_owned(),
            "/bin/agentbox-debug-entrypoint".to_owned(),
        ]);
    }

    if let Some(mount_arg) = options.debug_guest_init {
        args.extend(["--volume".to_owned(), mount_arg]);
    }

    args.extend([
        crate::DEFAULT_IMAGE.to_owned(),
        INTERACTIVE_SHELL.to_owned(),
        "-l".to_owned(),
    ]);
    args
}

fn raw_disk() -> RawNixDisk {
    RawNixDisk {
        path: PathBuf::from("/tmp/state/agentbox/project/libkrun-nix.raw"),
        id: "agentbox-nix".to_owned(),
        label: RAW_NIX_DISK_LABEL.to_owned(),
        size_bytes: RAW_NIX_DISK_SIZE_BYTES,
        status: RawNixDiskStatus::Reused,
    }
}

fn raw_container_disk() -> RawContainerDisk {
    RawContainerDisk {
        path: PathBuf::from("/tmp/state/agentbox/project/libkrun-containers.raw"),
        id: "agentbox-containers".to_owned(),
        label: RAW_CONTAINER_DISK_LABEL.to_owned(),
        size_bytes: RAW_CONTAINER_DISK_SIZE_BYTES,
        status: RawContainerDiskStatus::Reused,
    }
}

fn debug_entrypoint() -> DebugEntrypointMount {
    DebugEntrypointMount {
        source: PathBuf::from("/tmp/debug-entrypoint.sh"),
        mount_arg: "/tmp/debug-entrypoint.sh:/bin/agentbox-debug-entrypoint:ro".to_owned(),
        target: "/bin/agentbox-debug-entrypoint",
    }
}

fn debug_guest_init() -> DebugGuestInitMount {
    DebugGuestInitMount {
        source: PathBuf::from("/tmp/agentbox-guest-init"),
        mount_arg: "/tmp/agentbox-guest-init:/nix/store/hash-agentbox/bin/agentbox-guest-init:ro"
            .to_owned(),
        target: "/nix/store/hash-agentbox/bin/agentbox-guest-init".to_owned(),
    }
}

fn build_args(raw_nix_disk: &RawNixDisk, raw_container_disk: &RawContainerDisk) -> Vec<String> {
    build_args_with_options(
        raw_nix_disk,
        raw_container_disk,
        false,
        Some(16),
        None,
        None,
    )
}

fn build_args_with_tsi(
    raw_nix_disk: &RawNixDisk,
    raw_container_disk: &RawContainerDisk,
    tsi: bool,
) -> Vec<String> {
    build_args_with_options(raw_nix_disk, raw_container_disk, tsi, Some(16), None, None)
}

fn build_args_with_cpu_count(
    raw_nix_disk: &RawNixDisk,
    raw_container_disk: &RawContainerDisk,
    cpu_count: Option<u32>,
) -> Vec<String> {
    build_args_with_options(
        raw_nix_disk,
        raw_container_disk,
        false,
        cpu_count,
        None,
        None,
    )
}

fn build_args_with_debug_entrypoint(
    raw_nix_disk: &RawNixDisk,
    raw_container_disk: &RawContainerDisk,
    debug_entrypoint: Option<&DebugEntrypointMount>,
) -> Vec<String> {
    build_args_with_options(
        raw_nix_disk,
        raw_container_disk,
        false,
        Some(16),
        debug_entrypoint,
        None,
    )
}

fn build_args_with_debug_guest_init(
    raw_nix_disk: &RawNixDisk,
    raw_container_disk: &RawContainerDisk,
    debug_guest_init: Option<&DebugGuestInitMount>,
) -> Vec<String> {
    build_args_with_options(
        raw_nix_disk,
        raw_container_disk,
        false,
        Some(16),
        None,
        debug_guest_init,
    )
}

fn build_args_with_options(
    raw_nix_disk: &RawNixDisk,
    raw_container_disk: &RawContainerDisk,
    tsi: bool,
    cpu_count: Option<u32>,
    debug_entrypoint: Option<&DebugEntrypointMount>,
    debug_guest_init: Option<&DebugGuestInitMount>,
) -> Vec<String> {
    build_args_with_full_options(
        raw_nix_disk,
        raw_container_disk,
        tsi,
        cpu_count,
        false,
        false,
        debug_entrypoint,
        debug_guest_init,
    )
}

fn build_args_with_guest_diagnostics(
    raw_nix_disk: &RawNixDisk,
    raw_container_disk: &RawContainerDisk,
    guest_profile: bool,
    guest_debug: bool,
) -> Vec<String> {
    build_args_with_full_options(
        raw_nix_disk,
        raw_container_disk,
        false,
        Some(16),
        guest_profile,
        guest_debug,
        None,
        None,
    )
}

fn build_run_args(
    raw_nix_disk: &RawNixDisk,
    raw_container_disk: &RawContainerDisk,
) -> crate::podman::run::RunArgs {
    build_run_args_with_options(
        raw_nix_disk,
        raw_container_disk,
        false,
        Some(16),
        false,
        false,
        None,
        None,
    )
}

fn build_run_args_with_options(
    raw_nix_disk: &RawNixDisk,
    raw_container_disk: &RawContainerDisk,
    tsi: bool,
    cpu_count: Option<u32>,
    guest_profile: bool,
    guest_debug: bool,
    debug_entrypoint: Option<&DebugEntrypointMount>,
    debug_guest_init: Option<&DebugGuestInitMount>,
) -> crate::podman::run::RunArgs {
    let task_volumes = default_task_volumes();
    build_libkrun_task_run_args(crate::runtime::libkrun::task::LibkrunTaskPodmanSpec {
        image: crate::DEFAULT_IMAGE,
        container_name: "project-random",
        hostname: "project-agentbox",
        task_volumes: &task_volumes,
        raw_nix_disk,
        raw_container_disk,
        host_uid: 1001,
        host_gid: 1002,
        ram_mib: 8192,
        cpu_count,
        tsi,
        guest_profile,
        guest_debug,
        debug_entrypoint,
        debug_guest_init,
    })
    .expect("libkrun task args should build")
}

fn build_args_with_full_options(
    raw_nix_disk: &RawNixDisk,
    raw_container_disk: &RawContainerDisk,
    tsi: bool,
    cpu_count: Option<u32>,
    guest_profile: bool,
    guest_debug: bool,
    debug_entrypoint: Option<&DebugEntrypointMount>,
    debug_guest_init: Option<&DebugGuestInitMount>,
) -> Vec<String> {
    let task_volumes = default_task_volumes();
    build_libkrun_task_podman_args(crate::runtime::libkrun::task::LibkrunTaskPodmanSpec {
        image: crate::DEFAULT_IMAGE,
        container_name: "project-random",
        hostname: "project-agentbox",
        task_volumes: &task_volumes,
        raw_nix_disk,
        raw_container_disk,
        host_uid: 1001,
        host_gid: 1002,
        ram_mib: 8192,
        cpu_count,
        tsi,
        guest_profile,
        guest_debug,
        debug_entrypoint,
        debug_guest_init,
    })
    .expect("libkrun task args should build")
}
