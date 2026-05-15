use crate::podman::run::{RunArgOwner, RunSpec};

pub(crate) const OCI_OWNER: RunArgOwner = RunArgOwner::new("runtime.libkrun.oci");
pub(crate) const LIBKRUN_HANDLER_ANNOTATION: &str = "run.oci.handler=krun";

pub(crate) fn append_oci_args(run: &mut RunSpec) {
    run.option(OCI_OWNER, "--runtime", "crun");
    run.option(OCI_OWNER, "--annotation", LIBKRUN_HANDLER_ANNOTATION);
}
