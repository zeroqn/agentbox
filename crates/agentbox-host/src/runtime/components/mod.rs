pub(crate) mod allocator {
    use crate::podman::run::{RunArgOwner, RunSpec};

    pub(crate) const NIX_ALLOCATOR_OWNER: RunArgOwner = RunArgOwner::new("runtime.allocator");
    pub(crate) const NIX_ALLOCATOR_ENV: &str = "AGENTBOX_NIX_ALLOCATOR";
    pub(crate) const HARDENED_ALLOCATOR_VALUE: &str = "hardened";
    pub(crate) const HARDENED_ALLOCATOR_ENV: &str = "AGENTBOX_NIX_ALLOCATOR=hardened";

    pub(crate) fn append_hardened_allocator_env(run: &mut RunSpec, hardened: bool) {
        if hardened {
            run.option(NIX_ALLOCATOR_OWNER, "--env", HARDENED_ALLOCATOR_ENV);
        }
    }

    pub(crate) fn hardened_allocator_env_pair(hardened: bool) -> Option<(String, String)> {
        hardened.then(|| {
            (
                NIX_ALLOCATOR_ENV.to_owned(),
                HARDENED_ALLOCATOR_VALUE.to_owned(),
            )
        })
    }
}
pub(crate) mod diagnostics;
pub(crate) mod identity;
pub(crate) mod volumes;
