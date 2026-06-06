//! Guest-init override contribution validation.

use anyhow::Result;
use std::path::Path;

use super::super::model::GuestInitOverrideMount;

pub(crate) fn validate_guest_init_override_mount(
    mount: &GuestInitOverrideMount,
    exec_path: &str,
) -> Result<()> {
    if !mount.source.is_absolute() {
        anyhow::bail!(
            "loftd guest-init override bind source '{}' must be absolute",
            mount.source.display()
        );
    }
    if !Path::new(&mount.target).is_absolute() {
        anyhow::bail!(
            "loftd guest-init override bind target '{}' must be absolute",
            mount.target
        );
    }
    if !mount.read_only {
        anyhow::bail!("loftd guest-init override bind must be read-only");
    }
    if mount.target != exec_path {
        anyhow::bail!(
            "loftd guest-init override bind target '{}' must match guest-init exec path '{}'",
            mount.target,
            exec_path
        );
    }
    Ok(())
}
