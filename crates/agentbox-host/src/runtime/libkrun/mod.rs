mod components;
mod resize;
mod task;
#[cfg(test)]
mod task_tests;
pub(crate) use components::memory::parse_mem_gib_arg;
pub(crate) use resize::parse_raw_image_size_arg;

use anyhow::{Context, Result};
use std::env;
use std::process::{ExitCode, Stdio};

use crate::cli::{CommonOptions, LibkrunCommand, LibkrunOptions, LibkrunSubcommand, resolve_image};
use crate::naming::{derive_task_container_name, derive_task_hostname};
use crate::podman::command::run_podman;
use crate::runtime::components::volumes::prepare_task_volumes;
use crate::state::resolve_state_layout;

use components::cpu::resolve_libkrun_cpu_count;
use components::disk::{containers, nix};
use components::guest_init::resolve_guest_init_override_mount;
use components::memory::resolve_libkrun_ram_mib;
use task::{LibkrunTaskPodmanSpec, build_libkrun_task_podman_args};

pub(crate) fn run(common: CommonOptions, command: LibkrunCommand) -> Result<ExitCode> {
    match command.command {
        Some(LibkrunSubcommand::Resize(resize_options)) => {
            resize::run(common, command.run_options, resize_options)
        }
        None => run_task(common, command.run_options),
    }
}

fn run_task(common: CommonOptions, options: LibkrunOptions) -> Result<ExitCode> {
    let cwd = env::current_dir()
        .context("failed to resolve current directory")?
        .canonicalize()
        .context("failed to canonicalize current directory")?;
    let image = resolve_image(common.image.as_deref(), common.pull_latest)?;
    let state_layout = resolve_state_layout(&cwd)?;

    let guest_init_override = options
        .guest_init
        .as_deref()
        .map(|path| resolve_guest_init_override_mount(path, &image))
        .transpose()?;
    let raw_nix_disk = nix::raw_image::prepare(state_layout.root_dir())?;
    let raw_container_disk = containers::raw_image::prepare(state_layout.root_dir())?;
    let task_container_name = derive_task_container_name(&cwd);
    let task_hostname = derive_task_hostname(&cwd);
    let task_volumes = prepare_task_volumes(&cwd, &state_layout)?;
    let (host_uid, host_gid) = current_host_ids();
    let ram_mib = resolve_libkrun_ram_mib(options.mem_gib)?;
    let cpu_count = resolve_libkrun_cpu_count()?;

    let status = run_podman(
        build_libkrun_task_podman_args(LibkrunTaskPodmanSpec {
            image: &image,
            container_name: &task_container_name,
            hostname: &task_hostname,
            task_volumes: &task_volumes,
            raw_nix_disk: &raw_nix_disk,
            raw_container_disk: &raw_container_disk,
            host_uid,
            host_gid,
            ram_mib,
            cpu_count,
            tsi: options.tsi,
            guest_profile: common.profile,
            guest_debug: common.debug,
            enter_as_root: common.root,
            guest_init_override: guest_init_override.as_ref(),
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

#[cfg(test)]
mod tests {
    #[test]
    fn current_host_ids_are_available_for_kvm_drop_contract() {
        let (_uid, _gid) = crate::runtime::libkrun::current_host_ids();
    }
}
