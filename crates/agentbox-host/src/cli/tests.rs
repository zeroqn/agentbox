use crate::cli::{
    resolve_image_strategy, select_default_image, Cli, ContainerMode, ImageResolutionStrategy,
    LibkrunOptions, RuntimeCommand,
};
use crate::{DEFAULT_FALLBACK_IMAGE, DEFAULT_IMAGE};
use clap::error::ErrorKind;
use clap::CommandFactory;
use clap::Parser;
use std::path::Path;

#[test]
fn cli_accepts_no_arguments_as_default_libkrun() {
    let cli = Cli::try_parse_from(["agentbox"]).expect("no-arg invocation should parse");

    assert_eq!(cli.common_options().image, None);
    assert!(!cli.common_options().pull_latest);
    assert!(!cli.debug());
    assert!(!cli.common_options().profile);
    assert_eq!(
        cli.runtime_command_or_default(),
        RuntimeCommand::Libkrun(LibkrunOptions::default())
    );
}

#[test]
fn cli_supports_long_help() {
    let err = Cli::try_parse_from(["agentbox", "--help"]).expect_err("--help should short-circuit");
    assert_eq!(err.kind(), ErrorKind::DisplayHelp);
}

#[test]
fn cli_supports_short_help() {
    let err = Cli::try_parse_from(["agentbox", "-h"]).expect_err("-h should short-circuit");
    assert_eq!(err.kind(), ErrorKind::DisplayHelp);
}

#[test]
fn cli_supports_version() {
    let err =
        Cli::try_parse_from(["agentbox", "--version"]).expect_err("--version should short-circuit");
    assert_eq!(err.kind(), ErrorKind::DisplayVersion);
}

#[test]
fn cli_accepts_common_flags_at_top_level() {
    let cli = Cli::try_parse_from([
        "agentbox",
        "--image",
        "ghcr.io/example/agentbox:dev",
        "--pull-latest",
        "--profile",
        "--debug",
    ])
    .expect("common top-level flags should parse");
    let common = cli.common_options();

    assert_eq!(
        common.image.as_deref(),
        Some("ghcr.io/example/agentbox:dev")
    );
    assert!(common.pull_latest);
    assert!(common.profile);
    assert!(common.debug);
}

#[test]
fn cli_accepts_libkrun_subcommand_defaults() {
    let cli = Cli::try_parse_from(["agentbox", "libkrun"]).expect("libkrun should parse");

    assert_eq!(
        cli.runtime_command_or_default(),
        RuntimeCommand::Libkrun(LibkrunOptions::default())
    );
}

#[test]
fn cli_accepts_libkrun_options_under_libkrun_subcommand() {
    let cli = Cli::try_parse_from([
        "agentbox",
        "libkrun",
        "--tsi",
        "--mem",
        "8",
        "--guest-init",
        "./agentbox-guest-init",
    ])
    .expect("libkrun options should parse under libkrun subcommand");

    assert_eq!(
        cli.runtime_command_or_default(),
        RuntimeCommand::Libkrun(LibkrunOptions {
            tsi: true,
            mem_gib: Some(8),
            guest_init: Some(Path::new("./agentbox-guest-init").to_path_buf()),
        })
    );
}

#[test]
fn cli_accepts_container_subcommand_as_task_mode() {
    let cli = Cli::try_parse_from(["agentbox", "container"]).expect("container should parse");

    match cli.runtime_command_or_default() {
        RuntimeCommand::Container(options) => assert_eq!(options.mode(), ContainerMode::Task),
        other => panic!("expected container command, got {other:?}"),
    }
}

#[test]
fn cli_accepts_container_sidecar_subcommand() {
    let cli = Cli::try_parse_from(["agentbox", "container", "sidecar"])
        .expect("container sidecar should parse");

    match cli.runtime_command_or_default() {
        RuntimeCommand::Container(options) => assert_eq!(options.mode(), ContainerMode::Sidecar),
        other => panic!("expected container command, got {other:?}"),
    }
}

#[test]
fn cli_accepts_global_flags_before_and_after_container_subcommands() {
    let cases: &[&[&str]] = &[
        &["agentbox", "--debug", "container"],
        &["agentbox", "container", "--debug"],
        &["agentbox", "container", "sidecar", "--debug"],
        &[
            "agentbox",
            "--image",
            "ghcr.io/example/agentbox:dev",
            "container",
            "sidecar",
        ],
        &[
            "agentbox",
            "container",
            "--image",
            "ghcr.io/example/agentbox:dev",
            "sidecar",
        ],
    ];

    for case in cases {
        Cli::try_parse_from(*case).unwrap_or_else(|err| panic!("{case:?} should parse: {err}"));
    }
}

#[test]
fn cli_accepts_global_flags_around_libkrun_subcommand() {
    let before = Cli::try_parse_from(["agentbox", "--profile", "libkrun", "--mem", "4"])
        .expect("global flag before libkrun should parse");
    let after = Cli::try_parse_from(["agentbox", "libkrun", "--profile", "--mem", "4"])
        .expect("global flag after libkrun should parse");

    assert!(before.common_options().profile);
    assert!(after.common_options().profile);
}

fn removed_runtime_flags() -> [&'static str; 6] {
    [
        concat!("--", "native"),
        concat!("--", "libkrun"),
        concat!("--", "sidecar", "-", "only"),
        concat!("--", "disable", "-", "nix", "-", "sidecar"),
        concat!("--", "libkrun", "-", "debug", "-", "entrypoint"),
        concat!("--", "libkrun", "-", "debug", "-", "guest", "-", "init"),
    ]
}

#[test]
fn cli_rejects_removed_runtime_selector_and_control_flags() {
    for flag in removed_runtime_flags() {
        let err = Cli::try_parse_from(["agentbox", flag])
            .expect_err(&format!("{flag} should be rejected"));
        assert_eq!(err.kind(), ErrorKind::UnknownArgument, "{flag}");
    }
}

#[test]
fn cli_rejects_runtime_owned_libkrun_args_at_top_level() {
    let cases: &[&[&str]] = &[
        &["agentbox", "--mem", "8"],
        &["agentbox", "--tsi"],
        &["agentbox", "--guest-init", "./agentbox-guest-init"],
    ];

    for case in cases {
        let err = Cli::try_parse_from(*case).expect_err("top-level libkrun option should fail");
        assert_eq!(err.kind(), ErrorKind::UnknownArgument, "{case:?}");
    }
}

#[test]
fn cli_rejects_invalid_mem_under_libkrun_subcommand() {
    let zero = Cli::try_parse_from(["agentbox", "libkrun", "--mem", "0"])
        .expect_err("zero --mem should be rejected");
    assert_eq!(zero.kind(), ErrorKind::ValueValidation);

    let suffix = Cli::try_parse_from(["agentbox", "libkrun", "--mem", "8g"])
        .expect_err("suffix --mem should be rejected");
    assert_eq!(suffix.kind(), ErrorKind::ValueValidation);
}

#[test]
fn cli_rejects_previous_removed_flags() {
    for flag in [
        "--task-native",
        "--use-passt",
        "--host-nix-overlay",
        "--sync-nix-root",
        "--nix-sidecar",
        "--task-kvm",
    ] {
        let err = Cli::try_parse_from(["agentbox", flag])
            .expect_err(&format!("{flag} should be rejected"));
        assert_eq!(err.kind(), ErrorKind::UnknownArgument, "{flag}");
    }
}

#[test]
fn select_default_image_prefers_localhost_when_available() {
    assert_eq!(select_default_image(true), DEFAULT_IMAGE);
}

#[test]
fn select_default_image_uses_ghcr_fallback_when_localhost_missing() {
    assert_eq!(select_default_image(false), DEFAULT_FALLBACK_IMAGE);
}

#[test]
fn resolve_image_strategy_prefers_explicit_image_even_with_pull_latest() {
    let strategy = resolve_image_strategy(Some("ghcr.io/example/agentbox:dev"), true);
    assert_eq!(
        strategy,
        ImageResolutionStrategy::Explicit("ghcr.io/example/agentbox:dev".to_owned())
    );
}

#[test]
fn resolve_image_strategy_uses_pull_latest_when_requested() {
    let strategy = resolve_image_strategy(None, true);
    assert_eq!(strategy, ImageResolutionStrategy::PullLatestGhcr);
}

#[test]
fn resolve_image_strategy_defaults_to_local_preference() {
    let strategy = resolve_image_strategy(None, false);
    assert_eq!(strategy, ImageResolutionStrategy::PreferLocalhostFallback);
}

#[test]
fn clap_command_definition_is_valid() {
    Cli::command().debug_assert();
}
