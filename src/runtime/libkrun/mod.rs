pub(crate) mod containers;
mod cpu;
mod memory;
pub(crate) mod nix;
mod raw_disk;
pub(crate) use memory::parse_mem_gib_arg;

use anyhow::{Context, Result};
use std::env;
use std::path::{Path, PathBuf};
use std::process::{ExitCode, Stdio};

use crate::cli::{resolve_image, Cli};
use crate::mounts::format::{format_mount_arg, format_mount_arg_with_options};
use crate::mounts::{
    prepare_host_codex_mount, prepare_project_cargo_mount, prepare_shared_sccache_mount,
};
use crate::podman::command::run_podman;
use crate::state::resolve_state_layout;
use crate::{derive_task_container_name, derive_task_hostname, CONTAINER_WORKDIR};

use cpu::resolve_libkrun_cpu_count;
use memory::resolve_libkrun_ram_mib;
use task::{build_libkrun_task_podman_args, LibkrunTaskPodmanSpec};

pub(crate) fn run(cli: Cli) -> Result<ExitCode> {
    let cwd = env::current_dir()
        .context("failed to resolve current directory")?
        .canonicalize()
        .context("failed to canonicalize current directory")?;
    let image = resolve_image(cli.image.as_deref(), cli.pull_latest)?;
    let state_layout = resolve_state_layout(&cwd)?;

    let debug_entrypoint = cli
        .libkrun_debug_entrypoint
        .as_deref()
        .map(resolve_debug_entrypoint_mount)
        .transpose()?;
    let raw_nix_disk = nix::raw_image::prepare(state_layout.root_dir())?;
    let raw_container_disk = containers::raw_image::prepare(state_layout.root_dir())?;
    let task_container_name = derive_task_container_name(&cwd);
    let task_hostname = derive_task_hostname(&cwd);
    let workspace_mount = format_mount_arg(&cwd, CONTAINER_WORKDIR)?;
    let codex_mount = prepare_host_codex_mount()?;
    let cargo_mount = prepare_project_cargo_mount(state_layout.root_dir())?;
    let sccache_mount = prepare_shared_sccache_mount(&state_layout.sccache_dir())?;
    let (host_uid, host_gid) = current_host_ids();
    let ram_mib = resolve_libkrun_ram_mib(cli.mem_gib)?;
    let cpu_count = resolve_libkrun_cpu_count()?;

    let status = run_podman(
        build_libkrun_task_podman_args(LibkrunTaskPodmanSpec {
            image: &image,
            container_name: &task_container_name,
            hostname: &task_hostname,
            workspace_mount: &workspace_mount,
            codex_mount: &codex_mount,
            cargo_mount: &cargo_mount,
            sccache_mount: &sccache_mount,
            raw_nix_disk: &raw_nix_disk,
            raw_container_disk: &raw_container_disk,
            host_uid,
            host_gid,
            ram_mib,
            cpu_count,
            tsi: cli.tsi,
            debug_entrypoint: debug_entrypoint.as_ref(),
        })?,
        Stdio::inherit(),
        Stdio::inherit(),
        Stdio::inherit(),
        "failed to start podman libkrun task",
    )?;

    let code = status.code().unwrap_or(1);
    Ok(ExitCode::from(u8::try_from(code).unwrap_or(1)))
}

fn current_host_ids() -> (u32, u32) {
    (unsafe { libc::getuid() }, unsafe { libc::getgid() })
}

const LIBKRUN_DEBUG_ENTRYPOINT_TARGET: &str = "/bin/agentbox-debug-entrypoint";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DebugEntrypointMount {
    source: PathBuf,
    mount_arg: String,
    target: &'static str,
}

fn resolve_debug_entrypoint_mount(path: &Path) -> Result<DebugEntrypointMount> {
    let source = path.canonicalize().with_context(|| {
        format!(
            "failed to resolve libkrun debug entrypoint '{}'",
            path.display()
        )
    })?;
    if !source.is_file() {
        anyhow::bail!(
            "libkrun debug entrypoint '{}' is not a regular file",
            source.display()
        );
    }

    let mount_arg =
        format_mount_arg_with_options(&source, LIBKRUN_DEBUG_ENTRYPOINT_TARGET, Some("ro"))?;

    Ok(DebugEntrypointMount {
        source,
        mount_arg,
        target: LIBKRUN_DEBUG_ENTRYPOINT_TARGET,
    })
}

mod task {
    use anyhow::Result;

    use super::DebugEntrypointMount;
    use crate::runtime::libkrun::containers::raw_image::RawContainerDisk;
    use crate::runtime::libkrun::nix::raw_image::RawNixDisk;
    use crate::{CONTAINER_SCCACHE_DIR, CONTAINER_TMP_TMPFS, CONTAINER_WORKDIR, INTERACTIVE_SHELL};

    pub(crate) const LIBKRUN_HANDLER_ANNOTATION: &str = "run.oci.handler=krun";
    pub(crate) const LIBKRUN_NIX_OVERLAY_ENV: &str = "AGENTBOX_LIBKRUN_NIX_OVERLAY=1";
    pub(crate) const LIBKRUN_CONTAINERS_STORAGE_ENV: &str = "AGENTBOX_LIBKRUN_CONTAINERS_STORAGE=1";
    pub(crate) const LIBKRUN_KVM_DROP_TO_DEV_ENV: &str = "AGENTBOX_KVM_DROP_TO_DEV=1";
    pub(crate) const LIBKRUN_USE_PASST_ENV: &str = "AGENTBOX_LIBKRUN_USE_PASST=1";
    pub(crate) const LIBKRUN_USE_PASST_ANNOTATION: &str = "krun.use_passt=1";
    pub(crate) const LIBKRUN_RAM_MIB_ANNOTATION_PREFIX: &str = "krun.ram_mib=";
    pub(crate) const LIBKRUN_CPUS_ANNOTATION_PREFIX: &str = "krun.cpus=";
    pub(crate) const LIBKRUN_TSI_PROXY_ENV: &str = "no_proxy=1";

    pub(crate) struct LibkrunTaskPodmanSpec<'a> {
        pub(crate) image: &'a str,
        pub(crate) container_name: &'a str,
        pub(crate) hostname: &'a str,
        pub(crate) workspace_mount: &'a str,
        pub(crate) codex_mount: &'a str,
        pub(crate) cargo_mount: &'a str,
        pub(crate) sccache_mount: &'a str,
        pub(crate) raw_nix_disk: &'a RawNixDisk,
        pub(crate) raw_container_disk: &'a RawContainerDisk,
        pub(crate) host_uid: u32,
        pub(crate) host_gid: u32,
        pub(crate) ram_mib: u32,
        pub(crate) cpu_count: Option<u32>,
        pub(crate) tsi: bool,
        pub(crate) debug_entrypoint: Option<&'a DebugEntrypointMount>,
    }

    pub(crate) fn build_libkrun_task_podman_args(
        spec: LibkrunTaskPodmanSpec<'_>,
    ) -> Result<Vec<String>> {
        let disk = spec.raw_nix_disk;
        let container_disk = spec.raw_container_disk;
        let mut args = vec![
            "run".to_owned(),
            "--rm".to_owned(),
            "-it".to_owned(),
            "--name".to_owned(),
            spec.container_name.to_owned(),
            "--userns".to_owned(),
            "keep-id".to_owned(),
            "--user".to_owned(),
            "0:0".to_owned(),
            "--runtime".to_owned(),
            "crun".to_owned(),
            "--annotation".to_owned(),
            LIBKRUN_HANDLER_ANNOTATION.to_owned(),
            "--annotation".to_owned(),
            format!("{}{}", LIBKRUN_RAM_MIB_ANNOTATION_PREFIX, spec.ram_mib),
            "--annotation".to_owned(),
            format!("krun.disk.0.path={}", disk.path.display()),
            "--annotation".to_owned(),
            format!("krun.disk.0.id={}", disk.id),
            "--annotation".to_owned(),
            "krun.disk.0.readonly=false".to_owned(),
            "--annotation".to_owned(),
            format!("krun.disk.1.path={}", container_disk.path.display()),
            "--annotation".to_owned(),
            format!("krun.disk.1.id={}", container_disk.id),
            "--annotation".to_owned(),
            "krun.disk.1.readonly=false".to_owned(),
            "--workdir".to_owned(),
            CONTAINER_WORKDIR.to_owned(),
            "--hostname".to_owned(),
            spec.hostname.to_owned(),
            "--volume".to_owned(),
            spec.workspace_mount.to_owned(),
            "--volume".to_owned(),
            spec.codex_mount.to_owned(),
            "--volume".to_owned(),
            spec.cargo_mount.to_owned(),
            "--volume".to_owned(),
            spec.sccache_mount.to_owned(),
            "--env".to_owned(),
            format!("SCCACHE_DIR={CONTAINER_SCCACHE_DIR}"),
            "--env".to_owned(),
            LIBKRUN_NIX_OVERLAY_ENV.to_owned(),
            "--env".to_owned(),
            format!("AGENTBOX_LIBKRUN_NIX_DISK_ID={}", disk.id),
            "--env".to_owned(),
            format!("AGENTBOX_LIBKRUN_NIX_DISK_LABEL={}", disk.label),
            "--env".to_owned(),
            LIBKRUN_CONTAINERS_STORAGE_ENV.to_owned(),
            "--env".to_owned(),
            format!("AGENTBOX_LIBKRUN_CONTAINERS_DISK_ID={}", container_disk.id),
            "--env".to_owned(),
            format!(
                "AGENTBOX_LIBKRUN_CONTAINERS_DISK_LABEL={}",
                container_disk.label
            ),
            "--env".to_owned(),
            format!("AGENTBOX_HOST_UID={}", spec.host_uid),
            "--env".to_owned(),
            format!("AGENTBOX_HOST_GID={}", spec.host_gid),
            "--env".to_owned(),
            LIBKRUN_KVM_DROP_TO_DEV_ENV.to_owned(),
            "--tmpfs".to_owned(),
            CONTAINER_TMP_TMPFS.to_owned(),
        ];

        if let Some(cpu_count) = spec.cpu_count {
            args.push("--annotation".to_owned());
            args.push(format!("{}{}", LIBKRUN_CPUS_ANNOTATION_PREFIX, cpu_count));
        }

        if spec.tsi {
            args.push("--env".to_owned());
            args.push(LIBKRUN_TSI_PROXY_ENV.to_owned());
        } else {
            args.push("--env".to_owned());
            args.push(LIBKRUN_USE_PASST_ENV.to_owned());
            args.push("--annotation".to_owned());
            args.push(LIBKRUN_USE_PASST_ANNOTATION.to_owned());
        }

        if let Some(debug_entrypoint) = spec.debug_entrypoint {
            args.push("--volume".to_owned());
            args.push(debug_entrypoint.mount_arg.clone());
            args.push("--entrypoint".to_owned());
            args.push(debug_entrypoint.target.to_owned());
        }

        args.push(spec.image.to_owned());
        args.push(INTERACTIVE_SHELL.to_owned());
        args.push("-l".to_owned());

        Ok(args)
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::runtime::libkrun::containers::raw_image::{
            RawContainerDisk, RawContainerDiskStatus, RAW_CONTAINER_DISK_LABEL,
            RAW_CONTAINER_DISK_SIZE_BYTES,
        };
        use crate::runtime::libkrun::nix::raw_image::{
            RawNixDisk, RawNixDiskStatus, RAW_NIX_DISK_LABEL, RAW_NIX_DISK_SIZE_BYTES,
        };
        use std::path::PathBuf;

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
            assert!(args.contains(
                &"krun.disk.0.path=/tmp/state/agentbox/project/libkrun-nix.raw".to_owned()
            ));
            assert!(args.contains(&"krun.disk.0.id=agentbox-nix".to_owned()));
            assert!(args.contains(&"krun.disk.0.readonly=false".to_owned()));
            assert!(args.contains(
                &"krun.disk.1.path=/tmp/state/agentbox/project/libkrun-containers.raw".to_owned()
            ));
            assert!(args.contains(&"krun.disk.1.id=agentbox-containers".to_owned()));
            assert!(args.contains(&"krun.disk.1.readonly=false".to_owned()));
            assert!(args.contains(&LIBKRUN_NIX_OVERLAY_ENV.to_owned()));
            assert!(args.contains(&"AGENTBOX_LIBKRUN_NIX_DISK_ID=agentbox-nix".to_owned()));
            assert!(args.contains(&format!(
                "AGENTBOX_LIBKRUN_NIX_DISK_LABEL={RAW_NIX_DISK_LABEL}"
            )));
            assert!(args.contains(&LIBKRUN_CONTAINERS_STORAGE_ENV.to_owned()));
            assert!(args
                .contains(&"AGENTBOX_LIBKRUN_CONTAINERS_DISK_ID=agentbox-containers".to_owned()));
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
            assert!(
                args.contains(&"/tmp/state/agentbox/sccache:/home/dev/.cache/sccache".to_owned())
            );
            assert!(args.contains(&format!("SCCACHE_DIR={CONTAINER_SCCACHE_DIR}")));
            assert!(args.contains(&CONTAINER_TMP_TMPFS.to_owned()));
            assert!(args.contains(&LIBKRUN_USE_PASST_ENV.to_owned()));
            assert!(args.contains(&LIBKRUN_USE_PASST_ANNOTATION.to_owned()));
            assert!(!joined.contains("--memory"));
            assert!(!args.contains(&LIBKRUN_TSI_PROXY_ENV.to_owned()));
            assert_eq!(args[args.len() - 2], INTERACTIVE_SHELL);
            assert_eq!(args[args.len() - 1], "-l");
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
            assert!(!args.contains(&LIBKRUN_USE_PASST_ANNOTATION.to_owned()));
            assert!(!args.contains(&LIBKRUN_USE_PASST_ENV.to_owned()));
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
            assert!(args.contains(&LIBKRUN_USE_PASST_ANNOTATION.to_owned()));
            assert!(!joined.contains("--memory"));
        }

        #[test]
        fn libkrun_task_args_can_override_entrypoint_for_guest_debugging() {
            let disk = raw_disk();
            let container_disk = raw_container_disk();
            let debug_entrypoint = DebugEntrypointMount {
                source: PathBuf::from("/tmp/debug-entrypoint.sh"),
                mount_arg: "/tmp/debug-entrypoint.sh:/bin/agentbox-debug-entrypoint:ro".to_owned(),
                target: "/bin/agentbox-debug-entrypoint",
            };
            let args =
                build_args_with_debug_entrypoint(&disk, &container_disk, Some(&debug_entrypoint));
            let joined = args.join("\n");

            assert!(joined
                .contains("--volume\n/tmp/debug-entrypoint.sh:/bin/agentbox-debug-entrypoint:ro"));
            assert!(joined.contains("--entrypoint\n/bin/agentbox-debug-entrypoint"));
            assert!(joined.contains(&format!(
                "--entrypoint\n/bin/agentbox-debug-entrypoint\n{}\n{}\n-l",
                crate::DEFAULT_IMAGE,
                INTERACTIVE_SHELL
            )));
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

        fn build_args(
            raw_nix_disk: &RawNixDisk,
            raw_container_disk: &RawContainerDisk,
        ) -> Vec<String> {
            build_args_with_debug_entrypoint(raw_nix_disk, raw_container_disk, None)
        }

        fn build_args_with_tsi(
            raw_nix_disk: &RawNixDisk,
            raw_container_disk: &RawContainerDisk,
            tsi: bool,
        ) -> Vec<String> {
            build_args_with_options(raw_nix_disk, raw_container_disk, tsi, Some(16), None)
        }

        fn build_args_with_cpu_count(
            raw_nix_disk: &RawNixDisk,
            raw_container_disk: &RawContainerDisk,
            cpu_count: Option<u32>,
        ) -> Vec<String> {
            build_args_with_options(raw_nix_disk, raw_container_disk, false, cpu_count, None)
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
            )
        }

        fn build_args_with_options(
            raw_nix_disk: &RawNixDisk,
            raw_container_disk: &RawContainerDisk,
            tsi: bool,
            cpu_count: Option<u32>,
            debug_entrypoint: Option<&DebugEntrypointMount>,
        ) -> Vec<String> {
            build_libkrun_task_podman_args(LibkrunTaskPodmanSpec {
                image: crate::DEFAULT_IMAGE,
                container_name: "project-random",
                hostname: "project-agentbox",
                workspace_mount: "/tmp/project:/workspace",
                codex_mount: "/home/alice/.codex:/home/dev/.codex",
                cargo_mount: "/tmp/state/agentbox/project/cargo:/home/dev/.cargo",
                sccache_mount: "/tmp/state/agentbox/sccache:/home/dev/.cache/sccache",
                raw_nix_disk,
                raw_container_disk,
                host_uid: 1001,
                host_gid: 1002,
                ram_mib: 8192,
                cpu_count,
                tsi,
                debug_entrypoint,
            })
            .expect("libkrun task args should build")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_host_ids_are_available_for_kvm_drop_contract() {
        let (_uid, _gid) = current_host_ids();
    }
}
