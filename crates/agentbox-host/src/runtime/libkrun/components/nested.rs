use crate::podman::run::{RunArgOwner, RunSpec};

pub(crate) const NESTED_VIRT_OWNER: RunArgOwner = RunArgOwner::new("runtime.libkrun.nested");
pub(crate) const LIBKRUN_NESTED_VIRT_ANNOTATION: &str = "krun.nested_virt=1";

pub(crate) fn append_nested_virt_annotation(run: &mut RunSpec) {
    run.option(
        NESTED_VIRT_OWNER,
        "--annotation",
        LIBKRUN_NESTED_VIRT_ANNOTATION,
    );
}
