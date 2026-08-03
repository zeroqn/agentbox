use anyhow::Result;
use std::process::ExitCode;

pub mod granted;
mod guest_init;

pub fn granted_entrypoint() -> ExitCode {
    match granted::run(std::env::args_os().skip(1)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("loftd-granted: {err:#}");
            ExitCode::from(1)
        }
    }
}

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
