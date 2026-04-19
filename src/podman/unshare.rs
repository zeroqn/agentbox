pub fn build_podman_unshare_args(mut args: Vec<String>) -> Vec<String> {
    let mut wrapped = Vec::with_capacity(args.len() + 2);
    wrapped.push("unshare".to_owned());
    wrapped.push("podman".to_owned());
    wrapped.append(&mut args);
    wrapped
}
