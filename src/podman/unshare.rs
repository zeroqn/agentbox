use super::command::podman_debug_args;

pub fn build_podman_unshare_args(args: Vec<String>) -> Vec<String> {
    build_podman_unshare_args_with_inner_debug(args, !podman_debug_args().is_empty())
}

pub(crate) fn build_podman_unshare_args_with_inner_debug(
    mut args: Vec<String>,
    debug: bool,
) -> Vec<String> {
    let mut wrapped = Vec::with_capacity(args.len() + 3);
    wrapped.push("unshare".to_owned());
    wrapped.push("podman".to_owned());
    if debug {
        wrapped.push("--log-level=debug".to_owned());
    }
    wrapped.append(&mut args);
    wrapped
}
