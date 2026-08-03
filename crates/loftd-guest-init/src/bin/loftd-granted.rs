use std::process::ExitCode;

fn main() -> ExitCode {
    loftd_guest_init::granted_entrypoint()
}
