pub(crate) mod containers;
mod cpu;
mod memory;
mod network;
pub(crate) mod nix;
mod raw_disk;
mod task;
#[cfg(test)]
mod task_tests;
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
use crate::podman::command::{run_podman, run_podman_output};
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
    let debug_guest_init = cli
        .libkrun_debug_guest_init
        .as_deref()
        .map(|path| resolve_debug_guest_init_mount(path, &image))
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
            guest_profile: cli.profile,
            guest_debug: cli.debug,
            debug_entrypoint: debug_entrypoint.as_ref(),
            debug_guest_init: debug_guest_init.as_ref(),
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
const LIBKRUN_GUEST_INIT_BASENAME: &str = "agentbox-guest-init";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DebugEntrypointMount {
    source: PathBuf,
    mount_arg: String,
    target: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DebugGuestInitMount {
    source: PathBuf,
    mount_arg: String,
    target: String,
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

fn resolve_debug_guest_init_mount(path: &Path, image: &str) -> Result<DebugGuestInitMount> {
    let target = inspect_libkrun_guest_init_target(image)?;
    resolve_debug_guest_init_mount_to(path, &target)
}

fn resolve_debug_guest_init_mount_to(path: &Path, target: &str) -> Result<DebugGuestInitMount> {
    let source = path.canonicalize().with_context(|| {
        format!(
            "failed to resolve libkrun debug guest-init '{}'",
            path.display()
        )
    })?;
    if !source.is_file() {
        anyhow::bail!(
            "libkrun debug guest-init '{}' is not a regular file",
            source.display()
        );
    }

    let mount_arg = format_mount_arg_with_options(&source, target, Some("ro"))?;

    Ok(DebugGuestInitMount {
        source,
        mount_arg,
        target: target.to_owned(),
    })
}

fn inspect_libkrun_guest_init_target(image: &str) -> Result<String> {
    let args = vec![
        "image".to_owned(),
        "inspect".to_owned(),
        "--format".to_owned(),
        "{{index .Config.Entrypoint 0}}".to_owned(),
        image.to_owned(),
    ];
    let output = run_podman_output(
        args,
        "failed to inspect selected image entrypoint for libkrun debug guest-init",
    )
    .with_context(|| {
        format!(
            "selected image '{image}' must be local and inspectable for --libkrun-debug-guest-init"
        )
    })?;

    validate_libkrun_guest_init_target(image, output.trim())
}

fn validate_libkrun_guest_init_target(image: &str, target: &str) -> Result<String> {
    if target.is_empty() || target == "<no value>" {
        anyhow::bail!(
            concat!(
                "selected image '{}' does not define a first entrypoint element; ",
                "--libkrun-debug-guest-init requires an absolute agentbox-guest-init path"
            ),
            image
        );
    }

    let target_path = Path::new(target);
    if !target_path.is_absolute() {
        anyhow::bail!(
            concat!(
                "selected image '{}' first entrypoint element '{}' is not absolute; ",
                "--libkrun-debug-guest-init requires an absolute agentbox-guest-init path"
            ),
            image,
            target
        );
    }

    if target_path.file_name().and_then(|name| name.to_str()) != Some(LIBKRUN_GUEST_INIT_BASENAME) {
        anyhow::bail!(
            concat!(
                "selected image '{}' first entrypoint element '{}' does not point to {}; ",
                "--libkrun-debug-guest-init can only override ",
                "the image agentbox-guest-init binary"
            ),
            image,
            target,
            LIBKRUN_GUEST_INIT_BASENAME
        );
    }

    Ok(target.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_host_ids_are_available_for_kvm_drop_contract() {
        let (_uid, _gid) = current_host_ids();
    }

    #[test]
    fn validate_libkrun_guest_init_target_accepts_absolute_guest_init_path() {
        let target = validate_libkrun_guest_init_target(
            "localhost/agentbox:latest",
            "/nix/store/hash-agentbox/bin/agentbox-guest-init",
        )
        .expect("absolute guest-init target should be accepted");

        assert_eq!(target, "/nix/store/hash-agentbox/bin/agentbox-guest-init");
    }

    #[test]
    fn validate_libkrun_guest_init_target_rejects_empty_target() {
        assert_invalid_guest_init_target("");
        assert_invalid_guest_init_target("<no value>");
    }

    #[test]
    fn validate_libkrun_guest_init_target_rejects_relative_target() {
        assert_invalid_guest_init_target("agentbox-guest-init");
        assert_invalid_guest_init_target("sh");
        assert_invalid_guest_init_target("sh -c /nix/store/hash/bin/agentbox-guest-init");
    }

    #[test]
    fn validate_libkrun_guest_init_target_rejects_shell_entrypoints() {
        assert_invalid_guest_init_target("/bin/sh");
        assert_invalid_guest_init_target("/usr/bin/env");
    }

    #[test]
    fn validate_libkrun_guest_init_target_rejects_wrong_binary() {
        assert_invalid_guest_init_target("/nix/store/hash-agentbox/bin/not-agentbox-guest-init");
    }

    fn assert_invalid_guest_init_target(target: &str) {
        let err = validate_libkrun_guest_init_target("localhost/agentbox:latest", target)
            .expect_err("target should be rejected")
            .to_string();
        assert!(
            err.contains("--libkrun-debug-guest-init") || err.contains("agentbox-guest-init"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn resolve_debug_guest_init_mount_targets_image_guest_init_path() {
        let dir = tempfile::tempdir().expect("tempdir should be created");
        let source = dir.path().join("agentbox-guest-init");
        std::fs::write(&source, "#!/bin/sh\n").expect("debug guest-init should be written");

        let mount = resolve_debug_guest_init_mount_to(
            &source,
            "/nix/store/hash-agentbox/bin/agentbox-guest-init",
        )
        .expect("debug guest-init mount should resolve");

        assert_eq!(mount.source, source.canonicalize().unwrap());
        assert_eq!(
            mount.target,
            "/nix/store/hash-agentbox/bin/agentbox-guest-init"
        );
        assert!(mount
            .mount_arg
            .ends_with(":/nix/store/hash-agentbox/bin/agentbox-guest-init:ro"));
    }
}
