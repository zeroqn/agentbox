use anyhow::Result;

use crate::guest_init::components::home::identity::DevIdentity;

pub(in crate::guest_init) fn prepare(identity: &DevIdentity) -> Result<()> {
    crate::guest_init::components::rootless::idmap::prepare(identity)
}
