use crate::podman::run::{RunArgSource, RunSpec};

pub(crate) const LIBKRUN_USE_PASST_ENV: &str = "AGENTBOX_LIBKRUN_USE_PASST=1";
pub(crate) const LIBKRUN_USE_PASST_ANNOTATION: &str = "krun.use_passt=1";
pub(crate) const LIBKRUN_TSI_PROXY_ENV: &str = "no_proxy=1";
pub(crate) const LIBKRUN_TUN_DEVICE: &str = "/dev/net/tun:/dev/net/tun";

pub(crate) fn append_tun_device(run: &mut RunSpec) {
    run.option(RunArgSource::LibkrunNetwork, "--device", LIBKRUN_TUN_DEVICE);
}

pub(crate) fn append_mode_args(run: &mut RunSpec, tsi: bool) {
    if tsi {
        run.option(RunArgSource::LibkrunNetwork, "--env", LIBKRUN_TSI_PROXY_ENV);
    } else {
        run.option(RunArgSource::LibkrunNetwork, "--env", LIBKRUN_USE_PASST_ENV);
        run.option(
            RunArgSource::LibkrunNetwork,
            "--annotation",
            LIBKRUN_USE_PASST_ANNOTATION,
        );
    }
}
