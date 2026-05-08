use super::*;

#[test]
fn podman_debug_args_are_prefixed_before_subcommand() {
    assert_eq!(
        podman_args_for_debug(vec!["run".to_owned(), "image".to_owned()], true),
        vec![
            "--log-level=debug".to_owned(),
            "run".to_owned(),
            "image".to_owned(),
        ]
    );
}

#[test]
fn podman_debug_args_are_omitted_by_default() {
    assert_eq!(
        podman_args_for_debug(vec!["run".to_owned(), "image".to_owned()], false),
        vec!["run".to_owned(), "image".to_owned()]
    );
}
