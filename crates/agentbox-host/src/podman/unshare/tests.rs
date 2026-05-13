use super::*;

#[test]
fn podman_unshare_debug_args_are_prefixed_before_inner_subcommand() {
    assert_eq!(
        build_podman_unshare_args_with_inner_debug(
            vec![
                "image".to_owned(),
                "mount".to_owned(),
                "agentbox".to_owned(),
            ],
            true,
        ),
        vec![
            "unshare".to_owned(),
            "podman".to_owned(),
            "--log-level=debug".to_owned(),
            "image".to_owned(),
            "mount".to_owned(),
            "agentbox".to_owned(),
        ]
    );
}
