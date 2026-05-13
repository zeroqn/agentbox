use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};

static PODMAN_DEBUG: AtomicBool = AtomicBool::new(false);

pub fn set_podman_debug(enabled: bool) {
    PODMAN_DEBUG.store(enabled, Ordering::Relaxed);
}

pub fn podman_command() -> Command {
    let mut command = Command::new("podman");
    command.args(podman_args_for_debug(
        Vec::new(),
        PODMAN_DEBUG.load(Ordering::Relaxed),
    ));
    command
}

pub(in crate::podman) fn podman_debug_args() -> Vec<String> {
    podman_debug_args_for(PODMAN_DEBUG.load(Ordering::Relaxed))
}

fn podman_debug_args_for(debug: bool) -> Vec<String> {
    if debug {
        vec!["--log-level=debug".to_owned()]
    } else {
        Vec::new()
    }
}

fn podman_args_for_debug(mut args: Vec<String>, debug: bool) -> Vec<String> {
    let mut debug_args = podman_debug_args_for(debug);
    debug_args.append(&mut args);
    debug_args
}

#[cfg(test)]
mod tests;
