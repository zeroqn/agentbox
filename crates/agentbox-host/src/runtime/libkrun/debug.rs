use crate::podman::run::{RunArgOwner, RunSpec};
use crate::runtime::libkrun::{DebugEntrypointMount, DebugGuestInitMount};

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
