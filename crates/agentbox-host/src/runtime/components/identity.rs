use crate::podman::run::{RunArgOwner, RunSpec};

pub(crate) const USER_IDENTITY_OWNER: RunArgOwner = RunArgOwner::new("runtime.identity.userns");

pub(crate) fn append_userns_keep_id(run: &mut RunSpec) {
    run.option(USER_IDENTITY_OWNER, "--userns", "keep-id");
}
