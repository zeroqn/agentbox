use crate::podman::run::{RunArgOwner, RunSpec};
use crate::runtime::libkrun::containers::raw_image::RawContainerDisk;

pub(crate) const CONTAINERS_DISK_OWNER: RunArgOwner =
    RunArgOwner::new("runtime.libkrun.disk.containers");
pub(crate) const LIBKRUN_CONTAINERS_STORAGE_ENV: &str = "AGENTBOX_LIBKRUN_CONTAINERS_STORAGE=1";

pub(crate) fn append_disk_annotations(run: &mut RunSpec, disk: &RawContainerDisk) {
    run.option(
        CONTAINERS_DISK_OWNER,
        "--annotation",
        format!("krun.disk.1.path={}", disk.path.display()),
    );
    run.option(
        CONTAINERS_DISK_OWNER,
        "--annotation",
        format!("krun.disk.1.id={}", disk.id),
    );
    run.option(
        CONTAINERS_DISK_OWNER,
        "--annotation",
        "krun.disk.1.readonly=false",
    );
}

pub(crate) fn append_disk_env(run: &mut RunSpec, disk: &RawContainerDisk) {
    run.option(
        CONTAINERS_DISK_OWNER,
        "--env",
        LIBKRUN_CONTAINERS_STORAGE_ENV,
    );
    run.option(
        CONTAINERS_DISK_OWNER,
        "--env",
        format!("AGENTBOX_LIBKRUN_CONTAINERS_DISK_ID={}", disk.id),
    );
    run.option(
        CONTAINERS_DISK_OWNER,
        "--env",
        format!("AGENTBOX_LIBKRUN_CONTAINERS_DISK_LABEL={}", disk.label),
    );
}
