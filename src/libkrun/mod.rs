mod memory;
pub(crate) mod nix;
pub(crate) use memory::parse_mem_gib_arg;

use anyhow::Result;
use std::process::ExitCode;

use crate::cli::Cli;

pub(crate) fn run(_cli: Cli) -> Result<ExitCode> {
    nix::raw_image::prepare()?;
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn libkrun_fails_clearly_until_raw_image_exists() {
        let cli = Cli::parse_from(["agentbox", "--libkrun"]);
        let err = run(cli).expect_err("libkrun should fail until raw_image exists");
        let message = err.to_string();

        assert!(message.contains("libkrun mode is not available"));
        assert!(message.contains("raw_image"));
        assert!(message.contains("container mode remains the default"));
        assert!(message.contains("no sidecar/overlay/seeded fallback"));
    }
}
