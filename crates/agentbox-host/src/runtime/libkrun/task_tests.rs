use std::path::PathBuf;

use crate::runtime::components::diagnostics::{
    GUEST_DEBUG_ENV, GUEST_DIAGNOSTICS_OWNER, GUEST_PROFILE_ENV,
};
use crate::runtime::components::identity::USER_IDENTITY_OWNER;
use crate::runtime::components::volumes::{
    TaskVolumeMounts, SCCACHE_VOLUME_OWNER, WORKSPACE_VOLUME_OWNER,
};
use crate::runtime::libkrun::components::cpu::{CPU_OWNER, LIBKRUN_CPUS_ANNOTATION_PREFIX};
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
use crate::runtime::libkrun::components::guest_init::{
    GuestInitOverrideMount, GUEST_INIT_OVERRIDE_OWNER,
};
use crate::runtime::libkrun::components::host_identity::{
    HOST_IDENTITY_OWNER, LIBKRUN_KVM_DROP_TO_DEV_ENV,
};
use crate::runtime::libkrun::components::memory::MEMORY_OWNER;
use crate::runtime::libkrun::components::network;
use crate::runtime::libkrun::components::oci::{LIBKRUN_HANDLER_ANNOTATION, OCI_OWNER};
use crate::runtime::libkrun::task::{build_libkrun_task_podman_args, build_libkrun_task_run_args};
use crate::{CONTAINER_SCCACHE_DIR, CONTAINER_TMP_TMPFS, INTERACTIVE_SHELL};

#[test]
fn libkrun_task_args_match_ordered_default_passt_baseline() {
    let args = build_args(TaskOptions::default());
    assert_eq!(args, expected_args(ExpectedOptions::default()));
}

#[test]
fn libkrun_task_args_match_ordered_tsi_baseline() {
    let args = build_args(TaskOptions {
        tsi: true,
        ..Default::default()
    });

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
    let args = build_args(TaskOptions {
        cpu_count: None,
        ..Default::default()
    });

    assert_eq!(
        args,
        expected_args(ExpectedOptions {
            include_cpu: false,
            ..Default::default()
        })
    );
}

#[test]
fn libkrun_task_args_match_ordered_guest_init_override_baseline() {
    let guest_init = guest_init_override();
    let args = build_args(TaskOptions {
        guest_init: Some(guest_init.clone()),
        ..Default::default()
    });

    assert_eq!(
        args,
        expected_args(ExpectedOptions {
            guest_init: Some(guest_init.mount_arg),
            ..Default::default()
        })
    );
}

#[test]
fn libkrun_task_args_include_krun_disk_annotations_and_guest_overlay_env() {
    let args = build_args(TaskOptions::default());
    let joined = args.join("\n");

    assert_eq!(args[0], "run");
    assert!(args.contains(&"project-random".to_owned()));
    assert!(args.contains(&LIBKRUN_HANDLER_ANNOTATION.to_owned()));
    assert!(args.contains(&"krun.ram_mib=8192".to_owned()));
    assert!(args.contains(&"krun.cpus=16".to_owned()));
    assert!(args.contains(&"krun.disk.0.id=agentbox-nix".to_owned()));
    assert!(args.contains(&"krun.disk.1.id=agentbox-containers".to_owned()));
    assert!(joined.contains(&format!("--device\n{}", network::LIBKRUN_TUN_DEVICE)));
    assert!(args.contains(&LIBKRUN_NIX_OVERLAY_ENV.to_owned()));
    assert!(args.contains(&LIBKRUN_CONTAINERS_STORAGE_ENV.to_owned()));
    assert!(args.contains(&LIBKRUN_KVM_DROP_TO_DEV_ENV.to_owned()));
    assert!(joined.contains("--userns\nkeep-id"));
    assert!(joined.contains("--user\n0:0"));
    assert!(args.contains(&"/tmp/project:/workspace".to_owned()));
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
    let args = build_run_args(TaskOptions::default());

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
        CONTAINERS_DISK_OWNER,
        "--annotation",
        "krun.disk.1.id=agentbox-containers"
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
    assert!(args.contains_option_from(HOST_IDENTITY_OWNER, "--user", "0:0"));
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
fn libkrun_guest_init_override_is_owned_by_guest_init_override_owner() {
    let guest_init = guest_init_override();
    let args = build_run_args(TaskOptions {
        guest_init: Some(guest_init.clone()),
        ..Default::default()
    });

    assert!(args.contains_option_from(
        GUEST_INIT_OVERRIDE_OWNER,
        "--volume",
        &guest_init.mount_arg
    ));
}

#[test]
fn libkrun_tsi_proxy_env_is_owned_by_network_owner() {
    let args = build_run_args(TaskOptions {
        tsi: true,
        ..Default::default()
    });

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
    let args = build_run_args(TaskOptions {
        guest_profile: true,
        guest_debug: true,
        ..Default::default()
    });

    assert!(args.contains_option_from(GUEST_DIAGNOSTICS_OWNER, "--env", GUEST_PROFILE_ENV));
    assert!(args.contains_option_from(GUEST_DIAGNOSTICS_OWNER, "--env", GUEST_DEBUG_ENV));
}

#[test]
fn libkrun_task_args_exclude_container_sidecar_and_nix_proxy_paths() {
    let args = build_args(TaskOptions::default());
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
    let args = build_args(TaskOptions {
        tsi: true,
        ..Default::default()
    });
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
    let args = build_args(TaskOptions {
        cpu_count: None,
        ..Default::default()
    });
    let joined = args.join("\n");

    assert!(joined.contains("--annotation\nkrun.ram_mib=8192"));
    assert!(!args
        .iter()
        .any(|arg| arg.starts_with(LIBKRUN_CPUS_ANNOTATION_PREFIX)));
    assert!(args.contains(&network::LIBKRUN_USE_PASST_ANNOTATION.to_owned()));
    assert!(!joined.contains("--memory"));
}

#[test]
fn libkrun_task_args_can_override_guest_init_without_changing_entrypoint() {
    let guest_init = guest_init_override();
    let args = build_args(TaskOptions {
        guest_init: Some(guest_init),
        ..Default::default()
    });
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
    let args = build_args(TaskOptions {
        guest_profile: true,
        guest_debug: true,
        ..Default::default()
    });

    assert!(args.contains(&GUEST_PROFILE_ENV.to_owned()));
    assert!(args.contains(&GUEST_DEBUG_ENV.to_owned()));
}

#[test]
fn libkrun_task_args_omit_guest_profile_and_debug_env_by_default() {
    let args = build_args(TaskOptions::default());

    assert!(!args.contains(&GUEST_PROFILE_ENV.to_owned()));
    assert!(!args.contains(&GUEST_DEBUG_ENV.to_owned()));
}

#[derive(Debug, Clone)]
struct TaskOptions {
    tsi: bool,
    cpu_count: Option<u32>,
    guest_profile: bool,
    guest_debug: bool,
    guest_init: Option<GuestInitOverrideMount>,
}

impl Default for TaskOptions {
    fn default() -> Self {
        Self {
            tsi: false,
            cpu_count: Some(16),
            guest_profile: false,
            guest_debug: false,
            guest_init: None,
        }
    }
}

struct ExpectedOptions {
    tsi: bool,
    include_cpu: bool,
    guest_init: Option<String>,
}

impl Default for ExpectedOptions {
    fn default() -> Self {
        Self {
            tsi: false,
            include_cpu: true,
            guest_init: None,
        }
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

    if let Some(mount_arg) = options.guest_init {
        args.extend(["--volume".to_owned(), mount_arg]);
    }

    args.extend([
        crate::DEFAULT_IMAGE.to_owned(),
        INTERACTIVE_SHELL.to_owned(),
        "-l".to_owned(),
    ]);
    args
}

fn build_args(options: TaskOptions) -> Vec<String> {
    build_libkrun_task_podman_args(task_spec(options)).expect("libkrun task args should build")
}

fn build_run_args(options: TaskOptions) -> crate::podman::run::RunArgs {
    build_libkrun_task_run_args(task_spec(options)).expect("libkrun task args should build")
}

fn task_spec(
    options: TaskOptions,
) -> crate::runtime::libkrun::task::LibkrunTaskPodmanSpec<'static> {
    let task_volumes = Box::leak(Box::new(default_task_volumes()));
    let raw_nix_disk = Box::leak(Box::new(raw_disk()));
    let raw_container_disk = Box::leak(Box::new(raw_container_disk()));
    let guest_init_override = options
        .guest_init
        .map(|mount| Box::leak(Box::new(mount)) as &'static GuestInitOverrideMount);

    crate::runtime::libkrun::task::LibkrunTaskPodmanSpec {
        image: crate::DEFAULT_IMAGE,
        container_name: "project-random",
        hostname: "project-agentbox",
        task_volumes,
        raw_nix_disk,
        raw_container_disk,
        host_uid: 1001,
        host_gid: 1002,
        ram_mib: 8192,
        cpu_count: options.cpu_count,
        tsi: options.tsi,
        guest_profile: options.guest_profile,
        guest_debug: options.guest_debug,
        guest_init_override,
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

fn guest_init_override() -> GuestInitOverrideMount {
    GuestInitOverrideMount {
        source: PathBuf::from("/tmp/agentbox-guest-init"),
        mount_arg: "/tmp/agentbox-guest-init:/nix/store/hash-agentbox/bin/agentbox-guest-init:ro"
            .to_owned(),
        target: "/nix/store/hash-agentbox/bin/agentbox-guest-init".to_owned(),
    }
}
