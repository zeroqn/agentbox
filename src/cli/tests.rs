use super::*;
use clap::error::ErrorKind;
use clap::CommandFactory;

#[test]
fn cli_accepts_no_arguments() {
    let cli = Cli::try_parse_from(["agentbox"]).expect("no-arg invocation should parse");
    assert_eq!(cli.image, None);
    assert!(!cli.pull_latest);
    assert!(!cli.disable_nix_sidecar);
}

#[test]
fn default_runtime_enables_sidecar_when_no_flags_are_set() {
    let cli = Cli::try_parse_from(["agentbox"]).expect("no-arg invocation should parse");
    assert!(resolve_nix_sidecar_enabled(&cli, true));
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
fn cli_accepts_image_flag() {
    let cli = Cli::try_parse_from(["agentbox", "--image", "ghcr.io/example/agentbox:dev"])
        .expect("--image should parse");
    assert_eq!(cli.image.as_deref(), Some("ghcr.io/example/agentbox:dev"));
}

#[test]
fn cli_accepts_pull_latest_flag() {
    let cli =
        Cli::try_parse_from(["agentbox", "--pull-latest"]).expect("--pull-latest should parse");
    assert!(cli.pull_latest);
}

#[test]
fn cli_accepts_disable_nix_sidecar_flag() {
    let cli = Cli::try_parse_from(["agentbox", "--disable-nix-sidecar"])
        .expect("--disable-nix-sidecar should parse");
    assert!(cli.disable_nix_sidecar);
}

#[test]
fn disable_sidecar_flag_overrides_true_environment_value() {
    let cli = Cli::try_parse_from(["agentbox", "--disable-nix-sidecar"])
        .expect("--disable-nix-sidecar should parse");
    assert!(!resolve_nix_sidecar_enabled(&cli, true));
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
fn env_sidecar_flag_accepts_numeric_truthy_values() {
    assert!(parse_env_flag_value("AGENTBOX_NIX_SIDECAR", "1").expect("1 should parse"));
    assert!(!parse_env_flag_value("AGENTBOX_NIX_SIDECAR", "0").expect("0 should parse"));
}

#[test]
fn env_sidecar_flag_rejects_unknown_value() {
    let err = parse_env_flag_value("AGENTBOX_NIX_SIDECAR", "maybe")
        .expect_err("unknown env value should fail");
    assert!(err
        .to_string()
        .contains("environment variable 'AGENTBOX_NIX_SIDECAR' must be one of"));
}

#[test]
fn cli_rejects_removed_host_nix_overlay_flag() {
    let err = Cli::try_parse_from(["agentbox", "--host-nix-overlay"])
        .expect_err("--host-nix-overlay should be rejected");
    assert_eq!(err.kind(), ErrorKind::UnknownArgument);
}

#[test]
fn cli_rejects_removed_sync_nix_root_flag() {
    let err = Cli::try_parse_from(["agentbox", "--sync-nix-root"])
        .expect_err("--sync-nix-root should be rejected");
    assert_eq!(err.kind(), ErrorKind::UnknownArgument);
}

#[test]
fn cli_rejects_removed_nix_sidecar_flag() {
    let err = Cli::try_parse_from(["agentbox", "--nix-sidecar"])
        .expect_err("--nix-sidecar should be rejected");
    assert_eq!(err.kind(), ErrorKind::UnknownArgument);
}

#[test]
fn clap_command_definition_is_valid() {
    Cli::command().debug_assert();
}
