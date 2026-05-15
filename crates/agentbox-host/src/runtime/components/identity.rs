use crate::podman::run::{RunArgOwner, RunSpec};

pub(crate) const USER_IDENTITY_OWNER: RunArgOwner = RunArgOwner::new("runtime.identity.userns");
pub(crate) const ENTER_AS_ROOT_OWNER: RunArgOwner =
    RunArgOwner::new("runtime.identity.enter_as_root");
pub(crate) const ENTER_AS_ROOT_ENV: &str = "AGENTBOX_ENTER_AS_ROOT=1";

pub(crate) fn append_userns_keep_id(run: &mut RunSpec) {
    run.option(USER_IDENTITY_OWNER, "--userns", "keep-id");
}

pub(crate) fn append_root_user(run: &mut RunSpec) {
    run.option(ENTER_AS_ROOT_OWNER, "--user", "0:0");
}

pub(crate) fn append_enter_as_root_env(run: &mut RunSpec) {
    run.option(ENTER_AS_ROOT_OWNER, "--env", ENTER_AS_ROOT_ENV);
}
