use clap::Args;
use std::path::PathBuf;

use crate::runtime::parse_mem_gib_arg;

#[derive(Debug, Clone, Default, PartialEq, Eq, Args)]
pub struct LibkrunOptions {
    #[arg(
        long,
        help = "Use libkrun TSI/proxy networking instead of default passt",
        long_help = "Use libkrun TSI/proxy networking instead of default passt. By default libkrun mode enables passt with krun.use_passt=1; --tsi switches to the TSI/proxy environment path."
    )]
    pub tsi: bool,

    #[arg(
        long = "mem",
        value_name = "GiB",
        value_parser = parse_mem_gib_arg,
        help = "Set libkrun VM memory in GiB",
        long_help = "Set libkrun VM memory in integer GiB, emitted as a krun.ram_mib annotation. If omitted, agentbox derives a default from host memory."
    )]
    pub mem_gib: Option<u32>,

    #[arg(
        long = "guest-init",
        value_name = "PATH",
        help = "Override agentbox-guest-init in libkrun mode for guest debugging",
        long_help = "Bind-mount the host agentbox-guest-init binary read-only over the in-image guest-init path while preserving the normal image entrypoint and arguments. This lets guest-init fixes be tested without rebuilding the container image."
    )]
    pub guest_init: Option<PathBuf>,
}
