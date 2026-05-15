use anyhow::Result;

use crate::podman::run::{RunArgs, RunSpec, CORE};
use crate::runtime::components::{diagnostics, identity, volumes};
use crate::runtime::libkrun::components::debug::{DebugEntrypointMount, DebugGuestInitMount};
use crate::runtime::libkrun::components::disk::containers::podman as containers_podman;
use crate::runtime::libkrun::components::disk::containers::raw_image::RawContainerDisk;
use crate::runtime::libkrun::components::disk::nix::podman as nix_podman;
use crate::runtime::libkrun::components::disk::nix::raw_image::RawNixDisk;
use crate::runtime::libkrun::components::{cpu, debug, host_identity, memory, network, oci};
use crate::{CONTAINER_TMP_TMPFS, CONTAINER_WORKDIR, INTERACTIVE_SHELL};

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
    let mut run = RunSpec::new();

    run.args(CORE, ["run", "--rm", "-it"]);
    run.option(CORE, "--name", spec.container_name);
    identity::append_userns_keep_id(&mut run);
    host_identity::append_root_user(&mut run);
    oci::append_oci_args(&mut run);
    memory::append_ram_annotation(&mut run, spec.ram_mib);
    nix_podman::append_disk_annotations(&mut run, spec.raw_nix_disk);
    containers_podman::append_disk_annotations(&mut run, spec.raw_container_disk);
    network::append_tun_device(&mut run);
    run.option(CORE, "--workdir", CONTAINER_WORKDIR);
    run.option(CORE, "--hostname", spec.hostname);
    volumes::append_workspace(&mut run, spec.workspace_mount);
    volumes::append_codex(&mut run, spec.codex_mount);
    volumes::append_cargo(&mut run, spec.cargo_mount);
    volumes::append_sccache(&mut run, spec.sccache_mount);
    nix_podman::append_disk_env(&mut run, spec.raw_nix_disk);
    containers_podman::append_disk_env(&mut run, spec.raw_container_disk);
    host_identity::append_host_env(&mut run, spec.host_uid, spec.host_gid);
    run.option(CORE, "--tmpfs", CONTAINER_TMP_TMPFS);
    cpu::append_cpu_annotation(&mut run, spec.cpu_count);
    network::append_mode_args(&mut run, spec.tsi);
    diagnostics::append_guest_diagnostics(&mut run, spec.guest_profile, spec.guest_debug);
    debug::append_debug_args(&mut run, spec.debug_entrypoint, spec.debug_guest_init);
    run.arg(CORE, spec.image);
    run.args(CORE, [INTERACTIVE_SHELL, "-l"]);

    Ok(run.render())
}
