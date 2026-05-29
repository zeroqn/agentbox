use anyhow::Result;
use std::process::ExitCode;

mod guest_init;

pub fn entrypoint() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("loftd-guest-init: {err:#}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<()> {
    guest_init::entrypoint()
}
