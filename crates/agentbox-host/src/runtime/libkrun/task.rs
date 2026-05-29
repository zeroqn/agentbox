use anyhow::Result;

use crate::podman::run::{CORE, RunArgs, RunSpec};
use crate::runtime::components::volumes::TaskVolumeMounts;
use crate::runtime::components::{diagnostics, identity, volumes};
use crate::runtime::libkrun::components::disk::containers::podman as containers_podman;
use crate::runtime::libkrun::components::disk::containers::raw_image::RawContainerDisk;
use crate::runtime::libkrun::components::disk::nix::podman as nix_podman;
use crate::runtime::libkrun::components::disk::nix::raw_image::RawNixDisk;
use crate::runtime::libkrun::components::guest_init::GuestInitOverrideMount;
use crate::runtime::libkrun::components::{
    cpu, guest_init, host_identity, memory, nested, network, oci,
};
use crate::{CONTAINER_TMP_TMPFS, CONTAINER_WORKDIR, INTERACTIVE_SHELL};

pub(crate) struct LibkrunTaskPodmanSpec<'a> {
    pub(crate) image: &'a str,
    pub(crate) container_name: &'a str,
    pub(crate) hostname: &'a str,
    pub(crate) task_volumes: &'a TaskVolumeMounts,
    pub(crate) raw_nix_disk: &'a RawNixDisk,
    pub(crate) raw_container_disk: &'a RawContainerDisk,
    pub(crate) host_uid: u32,
    pub(crate) host_gid: u32,
    pub(crate) ram_mib: u32,
    pub(crate) cpu_count: Option<u32>,
    pub(crate) tsi: bool,
    pub(crate) publish_specs: &'a [String],
    pub(crate) guest_profile: bool,
    pub(crate) guest_debug: bool,
    pub(crate) enter_as_root: bool,
    pub(crate) guest_init_override: Option<&'a GuestInitOverrideMount>,
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
    nested::append_nested_virt_annotation(&mut run);
    nix_podman::append_disk_annotations(&mut run, spec.raw_nix_disk);
    containers_podman::append_disk_annotations(&mut run, spec.raw_container_disk);
    network::append_tun_device(&mut run);
    run.option(CORE, "--workdir", CONTAINER_WORKDIR);
    run.option(CORE, "--hostname", spec.hostname);
    volumes::append_task_volumes(&mut run, spec.task_volumes);
    nix_podman::append_disk_env(&mut run, spec.raw_nix_disk);
    containers_podman::append_disk_env(&mut run, spec.raw_container_disk);
    host_identity::append_host_env(&mut run, spec.host_uid, spec.host_gid);
    if spec.enter_as_root {
        identity::append_enter_as_root_env(&mut run);
    }
    run.option(CORE, "--tmpfs", CONTAINER_TMP_TMPFS);
    cpu::append_cpu_annotation(&mut run, spec.cpu_count);
    network::append_mode_args(&mut run, spec.tsi);
    network::append_publish_args(&mut run, spec.publish_specs);
    diagnostics::append_guest_diagnostics(&mut run, spec.guest_profile, spec.guest_debug);
    guest_init::append_guest_init_override_args(&mut run, spec.guest_init_override);
    run.arg(CORE, spec.image);
    run.args(CORE, [INTERACTIVE_SHELL, "-l"]);

    Ok(run.render())
}
