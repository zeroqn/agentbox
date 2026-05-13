use anyhow::{bail, Result};

use crate::guest_init::cli::EnterCommand;

pub(in crate::guest_init) fn enter(_command: EnterCommand) -> Result<()> {
    bail!("agentbox-guest-init container enter is parser-compatible but not wired in this image")
}
