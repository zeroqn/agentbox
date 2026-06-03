use anyhow::Result;

use crate::guest_init::cli::{
    GuestInitCommand, InternalSubcommand, NixSubcommand, PodmanSubcommand,
};

pub(in crate::guest_init) mod loftd;

pub(in crate::guest_init) fn run(command: GuestInitCommand) -> Result<()> {
    match command {
        GuestInitCommand::Enter(command) => loftd::enter(command.resolved_command()),
        GuestInitCommand::Internal(command) => match command.command {
            InternalSubcommand::Nix(nix) => match nix.command {
                NixSubcommand::Prep => {
                    crate::guest_init::components::nix::root::run_prep_to_status()
                }
                NixSubcommand::Wait => crate::guest_init::components::nix::user::wait_for_prep(),
            },
            InternalSubcommand::Podman(podman) => match podman.command {
                PodmanSubcommand::Prep => {
                    crate::guest_init::components::podman::root::run_prep_to_status()
                }
                PodmanSubcommand::Wait => {
                    crate::guest_init::components::podman::user::wait_for_prep()
                }
                PodmanSubcommand::ServiceWait => {
                    crate::guest_init::components::podman::user::wait_for_service()
                }
            },
            InternalSubcommand::Resize(resize) => {
                crate::guest_init::components::disk::resize::run(resize.target)
            }
        },
    }
}
