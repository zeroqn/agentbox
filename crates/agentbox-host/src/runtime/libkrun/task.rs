use anyhow::Result;

use crate::podman::run::{RunArgSource, RunArgs, RunSpec};
use crate::runtime::libkrun::containers::raw_image::RawContainerDisk;
use crate::runtime::libkrun::network;
use crate::runtime::libkrun::nix::raw_image::RawNixDisk;
use crate::runtime::libkrun::{DebugEntrypointMount, DebugGuestInitMount};
use crate::{CONTAINER_SCCACHE_DIR, CONTAINER_TMP_TMPFS, CONTAINER_WORKDIR, INTERACTIVE_SHELL};

pub(crate) const LIBKRUN_HANDLER_ANNOTATION: &str = "run.oci.handler=krun";
pub(crate) const LIBKRUN_NIX_OVERLAY_ENV: &str = "AGENTBOX_LIBKRUN_NIX_OVERLAY=1";
pub(crate) const LIBKRUN_CONTAINERS_STORAGE_ENV: &str = "AGENTBOX_LIBKRUN_CONTAINERS_STORAGE=1";
pub(crate) const LIBKRUN_KVM_DROP_TO_DEV_ENV: &str = "AGENTBOX_KVM_DROP_TO_DEV=1";
pub(crate) const LIBKRUN_RAM_MIB_ANNOTATION_PREFIX: &str = "krun.ram_mib=";
pub(crate) const LIBKRUN_CPUS_ANNOTATION_PREFIX: &str = "krun.cpus=";
pub(crate) const GUEST_PROFILE_ENV: &str = "AGENTBOX_GUEST_PROFILE=1";
pub(crate) const GUEST_DEBUG_ENV: &str = "AGENTBOX_GUEST_DEBUG=1";

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
    pub(crate) guest_profile: bool,
    pub(crate) guest_debug: bool,
    pub(crate) debug_entrypoint: Option<&'a DebugEntrypointMount>,
    pub(crate) debug_guest_init: Option<&'a DebugGuestInitMount>,
}

pub(crate) fn build_libkrun_task_podman_args(
    spec: LibkrunTaskPodmanSpec<'_>,
) -> Result<Vec<String>> {
    Ok(build_libkrun_task_run_args(spec)?.into_vec())
}

pub(crate) fn build_libkrun_task_run_args(spec: LibkrunTaskPodmanSpec<'_>) -> Result<RunArgs> {
    let disk = spec.raw_nix_disk;
    let container_disk = spec.raw_container_disk;
    let mut run = RunSpec::new();

    run.args(RunArgSource::Core, ["run", "--rm", "-it"]);
    run.option(RunArgSource::Core, "--name", spec.container_name);
    run.option(RunArgSource::UserIdentity, "--userns", "keep-id");
    run.option(RunArgSource::LibkrunHostIdentity, "--user", "0:0");
    run.option(RunArgSource::LibkrunOci, "--runtime", "crun");
    run.option(
        RunArgSource::LibkrunOci,
        "--annotation",
        LIBKRUN_HANDLER_ANNOTATION,
    );
    run.option(
        RunArgSource::LibkrunMemory,
        "--annotation",
        format!("{}{}", LIBKRUN_RAM_MIB_ANNOTATION_PREFIX, spec.ram_mib),
    );
    append_nix_disk_args(&mut run, disk);
    append_containers_disk_args(&mut run, container_disk);
    network::append_tun_device(&mut run);
    run.option(RunArgSource::Core, "--workdir", CONTAINER_WORKDIR);
    run.option(RunArgSource::Core, "--hostname", spec.hostname);
    run.option(
        RunArgSource::WorkspaceVolume,
        "--volume",
        spec.workspace_mount,
    );
    run.option(RunArgSource::CodexVolume, "--volume", spec.codex_mount);
    run.option(RunArgSource::CargoVolume, "--volume", spec.cargo_mount);
    run.option(RunArgSource::SccacheVolume, "--volume", spec.sccache_mount);
    run.option(
        RunArgSource::SccacheVolume,
        "--env",
        format!("SCCACHE_DIR={CONTAINER_SCCACHE_DIR}"),
    );
    append_nix_disk_env(&mut run, disk);
    append_containers_disk_env(&mut run, container_disk);
    run.option(
        RunArgSource::LibkrunHostIdentity,
        "--env",
        format!("AGENTBOX_HOST_UID={}", spec.host_uid),
    );
    run.option(
        RunArgSource::LibkrunHostIdentity,
        "--env",
        format!("AGENTBOX_HOST_GID={}", spec.host_gid),
    );
    run.option(
        RunArgSource::LibkrunHostIdentity,
        "--env",
        LIBKRUN_KVM_DROP_TO_DEV_ENV,
    );
    run.option(RunArgSource::Core, "--tmpfs", CONTAINER_TMP_TMPFS);

    if let Some(cpu_count) = spec.cpu_count {
        run.option(
            RunArgSource::LibkrunCpu,
            "--annotation",
            format!("{}{}", LIBKRUN_CPUS_ANNOTATION_PREFIX, cpu_count),
        );
    }

    network::append_mode_args(&mut run, spec.tsi);
    append_guest_diagnostics(&mut run, spec.guest_profile, spec.guest_debug);
    append_debug_args(&mut run, spec.debug_entrypoint, spec.debug_guest_init);
    run.arg(RunArgSource::Core, spec.image);
    run.args(RunArgSource::Core, [INTERACTIVE_SHELL, "-l"]);

    Ok(run.render())
}

fn append_nix_disk_args(run: &mut RunSpec, disk: &RawNixDisk) {
    run.option(
        RunArgSource::LibkrunNixDisk,
        "--annotation",
        format!("krun.disk.0.path={}", disk.path.display()),
    );
    run.option(
        RunArgSource::LibkrunNixDisk,
        "--annotation",
        format!("krun.disk.0.id={}", disk.id),
    );
    run.option(
        RunArgSource::LibkrunNixDisk,
        "--annotation",
        "krun.disk.0.readonly=false",
    );
}

fn append_containers_disk_args(run: &mut RunSpec, disk: &RawContainerDisk) {
    run.option(
        RunArgSource::LibkrunContainersDisk,
        "--annotation",
        format!("krun.disk.1.path={}", disk.path.display()),
    );
    run.option(
        RunArgSource::LibkrunContainersDisk,
        "--annotation",
        format!("krun.disk.1.id={}", disk.id),
    );
    run.option(
        RunArgSource::LibkrunContainersDisk,
        "--annotation",
        "krun.disk.1.readonly=false",
    );
}

fn append_nix_disk_env(run: &mut RunSpec, disk: &RawNixDisk) {
    run.option(
        RunArgSource::LibkrunNixDisk,
        "--env",
        LIBKRUN_NIX_OVERLAY_ENV,
    );
    run.option(
        RunArgSource::LibkrunNixDisk,
        "--env",
        format!("AGENTBOX_LIBKRUN_NIX_DISK_ID={}", disk.id),
    );
    run.option(
        RunArgSource::LibkrunNixDisk,
        "--env",
        format!("AGENTBOX_LIBKRUN_NIX_DISK_LABEL={}", disk.label),
    );
}

fn append_containers_disk_env(run: &mut RunSpec, disk: &RawContainerDisk) {
    run.option(
        RunArgSource::LibkrunContainersDisk,
        "--env",
        LIBKRUN_CONTAINERS_STORAGE_ENV,
    );
    run.option(
        RunArgSource::LibkrunContainersDisk,
        "--env",
        format!("AGENTBOX_LIBKRUN_CONTAINERS_DISK_ID={}", disk.id),
    );
    run.option(
        RunArgSource::LibkrunContainersDisk,
        "--env",
        format!("AGENTBOX_LIBKRUN_CONTAINERS_DISK_LABEL={}", disk.label),
    );
}

fn append_guest_diagnostics(run: &mut RunSpec, guest_profile: bool, guest_debug: bool) {
    if guest_profile {
        run.option(RunArgSource::GuestDiagnostics, "--env", GUEST_PROFILE_ENV);
    }

    if guest_debug {
        run.option(RunArgSource::GuestDiagnostics, "--env", GUEST_DEBUG_ENV);
    }
}

fn append_debug_args(
    run: &mut RunSpec,
    debug_entrypoint: Option<&DebugEntrypointMount>,
    debug_guest_init: Option<&DebugGuestInitMount>,
) {
    if let Some(debug_entrypoint) = debug_entrypoint {
        run.option(
            RunArgSource::LibkrunDebug,
            "--volume",
            debug_entrypoint.mount_arg.clone(),
        );
        run.option(
            RunArgSource::LibkrunDebug,
            "--entrypoint",
            debug_entrypoint.target,
        );
    }

    if let Some(debug_guest_init) = debug_guest_init {
        run.option(
            RunArgSource::LibkrunDebug,
            "--volume",
            debug_guest_init.mount_arg.clone(),
        );
    }
}
