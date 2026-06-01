use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use crate::AGENTBOX_GUEST_INIT_ENTRYPOINT;
use crate::podman::run::{RunArgOwner, RunSpec};
use crate::podman::volume::format_mount_arg_with_options;

pub(crate) const GUEST_INIT_OVERRIDE_OWNER: RunArgOwner =
    RunArgOwner::new("runtime.libkrun.guest_init_override");

pub(crate) fn append_guest_init_override_args(
    run: &mut RunSpec,
    guest_init_override: Option<&GuestInitOverrideMount>,
) {
    if let Some(guest_init_override) = guest_init_override {
        run.option(
            GUEST_INIT_OVERRIDE_OWNER,
            "--volume",
            guest_init_override.mount_arg.clone(),
        );
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GuestInitOverrideMount {
    pub(crate) source: PathBuf,
    pub(crate) mount_arg: String,
    pub(crate) target: String,
}

pub(crate) fn resolve_guest_init_override_mount(
    path: &Path,
    _image: &str,
) -> Result<GuestInitOverrideMount> {
    let target = resolve_libkrun_guest_init_target();
    resolve_guest_init_override_mount_to(path, &target)
}

pub(crate) fn resolve_libkrun_guest_init_target() -> String {
    AGENTBOX_GUEST_INIT_ENTRYPOINT.to_owned()
}

fn resolve_guest_init_override_mount_to(
    path: &Path,
    target: &str,
) -> Result<GuestInitOverrideMount> {
    let source = path.canonicalize().with_context(|| {
        format!(
            "failed to resolve libkrun guest-init override '{}'",
            path.display()
        )
    })?;
    if !source.is_file() {
        anyhow::bail!(
            "libkrun guest-init override '{}' is not a regular file",
            source.display()
        );
    }

    let mount_arg = format_mount_arg_with_options(&source, target, Some("ro"))?;

    Ok(GuestInitOverrideMount {
        source,
        mount_arg,
        target: target.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use crate::runtime::libkrun::components::guest_init::{
        resolve_guest_init_override_mount_to, resolve_libkrun_guest_init_target,
    };

    #[test]
    fn libkrun_guest_init_target_is_stable_in_shared_loftd_image() {
        assert_eq!(
            resolve_libkrun_guest_init_target(),
            "/bin/agentbox-guest-init"
        );
    }

    #[test]
    fn resolve_guest_init_override_mount_targets_image_guest_init_path() {
        let dir = tempfile::tempdir().expect("tempdir should be created");
        let source = dir.path().join("agentbox-guest-init");
        std::fs::write(&source, "#!/bin/sh\n").expect("guest-init override should be written");

        let mount = resolve_guest_init_override_mount_to(&source, "/bin/agentbox-guest-init")
            .expect("guest-init override mount should resolve");

        assert_eq!(mount.source, source.canonicalize().unwrap());
        assert_eq!(mount.target, "/bin/agentbox-guest-init");
        assert!(mount.mount_arg.ends_with(":/bin/agentbox-guest-init:ro"));
    }
}
