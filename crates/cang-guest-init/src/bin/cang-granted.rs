use std::process::ExitCode;

fn main() -> ExitCode {
    cang_guest_init::granted_entrypoint()
}
