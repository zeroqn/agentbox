use clap::{Args, ValueEnum};
use std::path::PathBuf;

use crate::runtime::parse_mem_gib_arg;

#[derive(Debug, Clone, PartialEq, Eq, Args)]
#[command(
    long_about = "Run experimental direct-libkrun microvm mode as a one-shot task with a clean task rootfs.",
    after_help = "Microvm notes:
  - experimental direct-libkrun path; separate from Podman-backed libkrun mode
  - each run gets a clean task rootfs materialized from the immutable image rootfs cache
  - --preserve-debug keeps the task rootfs and launch config for inspection after failures
  - persistent /nix and container-store disk images are reused per workspace
  - no direct microvm inbound port publishing is available in this experimental mode
  - outbound networking uses libkrun's current default path and needs real-VM smoke validation
  - terminal resize has the default virtio-console hook wired, but still needs real-VM smoke validation"
)]
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
    #[value(name = "btrfs-snapshot")]
    BtrfsSnapshot,
    Reflink,
    #[value(name = "fuse-overlay")]
    FuseOverlay,
}
