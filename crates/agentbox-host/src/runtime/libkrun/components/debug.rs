use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use crate::mounts::format::format_mount_arg_with_options;
use crate::podman::command::run_podman_output;
use crate::podman::run::{RunArgOwner, RunSpec};

pub(crate) const DEBUG_OWNER: RunArgOwner = RunArgOwner::new("runtime.libkrun.debug");

pub(crate) fn append_debug_args(
    run: &mut RunSpec,
    debug_entrypoint: Option<&DebugEntrypointMount>,
    debug_guest_init: Option<&DebugGuestInitMount>,
) {
    if let Some(debug_entrypoint) = debug_entrypoint {
        run.option(DEBUG_OWNER, "--volume", debug_entrypoint.mount_arg.clone());
        run.option(DEBUG_OWNER, "--entrypoint", debug_entrypoint.target);
    }

    if let Some(debug_guest_init) = debug_guest_init {
        run.option(DEBUG_OWNER, "--volume", debug_guest_init.mount_arg.clone());
    }
}

const LIBKRUN_DEBUG_ENTRYPOINT_TARGET: &str = "/bin/agentbox-debug-entrypoint";
const LIBKRUN_GUEST_INIT_BASENAME: &str = "agentbox-guest-init";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DebugEntrypointMount {
    pub(crate) source: PathBuf,
    pub(crate) mount_arg: String,
    pub(crate) target: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DebugGuestInitMount {
    pub(crate) source: PathBuf,
    pub(crate) mount_arg: String,
    pub(crate) target: String,
}

pub(crate) fn resolve_debug_entrypoint_mount(path: &Path) -> Result<DebugEntrypointMount> {
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

pub(crate) fn resolve_debug_guest_init_mount(
    path: &Path,
    image: &str,
) -> Result<DebugGuestInitMount> {
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
    use crate::runtime::libkrun::components::debug::{
        resolve_debug_guest_init_mount_to, validate_libkrun_guest_init_target,
    };

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
