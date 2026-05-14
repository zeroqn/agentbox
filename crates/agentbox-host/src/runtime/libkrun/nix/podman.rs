use crate::podman::run::{RunArgOwner, RunSpec};
use crate::runtime::libkrun::nix::raw_image::RawNixDisk;

pub(crate) const NIX_DISK_OWNER: RunArgOwner = RunArgOwner::new("runtime.libkrun.disk.nix");
pub(crate) const LIBKRUN_NIX_OVERLAY_ENV: &str = "AGENTBOX_LIBKRUN_NIX_OVERLAY=1";

pub(crate) fn append_disk_annotations(run: &mut RunSpec, disk: &RawNixDisk) {
    run.option(
        NIX_DISK_OWNER,
        "--annotation",
        format!("krun.disk.0.path={}", disk.path.display()),
    );
    run.option(
        NIX_DISK_OWNER,
        "--annotation",
        format!("krun.disk.0.id={}", disk.id),
    );
    run.option(NIX_DISK_OWNER, "--annotation", "krun.disk.0.readonly=false");
}

pub(crate) fn append_disk_env(run: &mut RunSpec, disk: &RawNixDisk) {
    run.option(NIX_DISK_OWNER, "--env", LIBKRUN_NIX_OVERLAY_ENV);
    run.option(
        NIX_DISK_OWNER,
        "--env",
        format!("AGENTBOX_LIBKRUN_NIX_DISK_ID={}", disk.id),
    );
    run.option(
        NIX_DISK_OWNER,
        "--env",
        format!("AGENTBOX_LIBKRUN_NIX_DISK_LABEL={}", disk.label),
    );
}
