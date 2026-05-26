use clap::{Args, ValueEnum};
use std::path::PathBuf;

use crate::runtime::parse_mem_gib_arg;

#[derive(Debug, Clone, PartialEq, Eq, Args)]
pub struct MicrovmOptions {
    #[arg(
        long = "storage",
        value_enum,
        default_value = "auto",
        help = "Select the experimental microvm task rootfs storage backend"
    )]
    pub storage: MicrovmStoragePolicy,

    #[arg(
        long = "guest-init",
        value_name = "PATH",
        help = "Override agentbox-guest-init in experimental microvm mode"
    )]
    pub guest_init: Option<PathBuf>,

    #[arg(
        long = "preserve-debug",
        help = "Preserve experimental microvm task debug state after exit"
    )]
    pub preserve_debug: bool,

    #[arg(
        long = "mem",
        value_name = "GiB",
        value_parser = parse_mem_gib_arg,
        help = "Set experimental microvm memory in GiB"
    )]
    pub mem_gib: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum MicrovmStoragePolicy {
    Auto,
    Btrfs,
    #[value(name = "fuse-overlay")]
    FuseOverlay,
}
