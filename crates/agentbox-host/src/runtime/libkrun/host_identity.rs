use crate::podman::run::{RunArgOwner, RunSpec};

pub(crate) const HOST_IDENTITY_OWNER: RunArgOwner =
    RunArgOwner::new("runtime.libkrun.host_identity");
pub(crate) const LIBKRUN_KVM_DROP_TO_DEV_ENV: &str = "AGENTBOX_KVM_DROP_TO_DEV=1";

pub(crate) fn append_root_user(run: &mut RunSpec) {
    run.option(HOST_IDENTITY_OWNER, "--user", "0:0");
}

pub(crate) fn append_host_env(run: &mut RunSpec, host_uid: u32, host_gid: u32) {
    run.option(
        HOST_IDENTITY_OWNER,
        "--env",
        format!("AGENTBOX_HOST_UID={host_uid}"),
    );
    run.option(
        HOST_IDENTITY_OWNER,
        "--env",
        format!("AGENTBOX_HOST_GID={host_gid}"),
    );
    run.option(HOST_IDENTITY_OWNER, "--env", LIBKRUN_KVM_DROP_TO_DEV_ENV);
}
