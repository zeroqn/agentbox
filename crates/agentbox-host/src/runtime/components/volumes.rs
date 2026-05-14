use crate::podman::run::{RunArgOwner, RunSpec};
use crate::CONTAINER_SCCACHE_DIR;

pub(crate) const WORKSPACE_VOLUME_OWNER: RunArgOwner = RunArgOwner::new("runtime.volume.workspace");
pub(crate) const CODEX_VOLUME_OWNER: RunArgOwner = RunArgOwner::new("runtime.volume.codex");
pub(crate) const CARGO_VOLUME_OWNER: RunArgOwner = RunArgOwner::new("runtime.volume.cargo");
pub(crate) const SCCACHE_VOLUME_OWNER: RunArgOwner = RunArgOwner::new("runtime.volume.sccache");

pub(crate) fn append_workspace(run: &mut RunSpec, mount: &str) {
    run.option(WORKSPACE_VOLUME_OWNER, "--volume", mount);
}

pub(crate) fn append_codex(run: &mut RunSpec, mount: &str) {
    run.option(CODEX_VOLUME_OWNER, "--volume", mount);
}

pub(crate) fn append_cargo(run: &mut RunSpec, mount: &str) {
    run.option(CARGO_VOLUME_OWNER, "--volume", mount);
}

pub(crate) fn append_sccache(run: &mut RunSpec, mount: &str) {
    run.option(SCCACHE_VOLUME_OWNER, "--volume", mount);
    run.option(
        SCCACHE_VOLUME_OWNER,
        "--env",
        format!("SCCACHE_DIR={CONTAINER_SCCACHE_DIR}"),
    );
}
