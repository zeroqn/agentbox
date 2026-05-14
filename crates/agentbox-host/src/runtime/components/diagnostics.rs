use crate::podman::run::{RunArgOwner, RunSpec};

pub(crate) const GUEST_DIAGNOSTICS_OWNER: RunArgOwner =
    RunArgOwner::new("runtime.guest.diagnostics");
pub(crate) const GUEST_PROFILE_ENV: &str = "AGENTBOX_GUEST_PROFILE=1";
pub(crate) const GUEST_DEBUG_ENV: &str = "AGENTBOX_GUEST_DEBUG=1";

pub(crate) fn append_guest_diagnostics(run: &mut RunSpec, guest_profile: bool, guest_debug: bool) {
    if guest_profile {
        run.option(GUEST_DIAGNOSTICS_OWNER, "--env", GUEST_PROFILE_ENV);
    }

    if guest_debug {
        run.option(GUEST_DIAGNOSTICS_OWNER, "--env", GUEST_DEBUG_ENV);
    }
}
