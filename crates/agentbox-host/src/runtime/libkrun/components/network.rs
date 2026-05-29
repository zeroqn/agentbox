use crate::podman::run::{RunArgOwner, RunSpec};

pub(crate) const NETWORK_OWNER: RunArgOwner = RunArgOwner::new("runtime.libkrun.network");

pub(crate) const LIBKRUN_USE_PASST_ENV: &str = "AGENTBOX_LIBKRUN_USE_PASST=1";
pub(crate) const LIBKRUN_USE_PASST_ANNOTATION: &str = "krun.use_passt=1";
pub(crate) const LIBKRUN_TSI_PROXY_ENV: &str = "no_proxy=1";
pub(crate) const LIBKRUN_TUN_DEVICE: &str = "/dev/net/tun:/dev/net/tun";

pub(crate) fn append_tun_device(run: &mut RunSpec) {
    run.option(NETWORK_OWNER, "--device", LIBKRUN_TUN_DEVICE);
}

pub(crate) fn append_mode_args(run: &mut RunSpec, tsi: bool) {
    if tsi {
        run.option(NETWORK_OWNER, "--env", LIBKRUN_TSI_PROXY_ENV);
    } else {
        run.option(NETWORK_OWNER, "--env", LIBKRUN_USE_PASST_ENV);
        run.option(NETWORK_OWNER, "--annotation", LIBKRUN_USE_PASST_ANNOTATION);
    }
}

pub(crate) fn append_publish_args(run: &mut RunSpec, publish_specs: &[String]) {
    for spec in publish_specs {
        run.option(NETWORK_OWNER, "--publish", spec);
    }
}
