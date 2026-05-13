mod container;
mod libkrun;

use anyhow::Result;
use std::process::ExitCode;

use crate::cli::{env_flag_enabled, Cli};
use crate::DEFAULT_NIX_SIDECAR_ENABLED;

pub(crate) use libkrun::parse_mem_gib_arg;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RuntimeMode {
    Container,
    Libkrun,
}

pub(crate) fn run(cli: Cli) -> Result<ExitCode> {
    match resolve_runtime_mode(&cli)? {
        RuntimeMode::Container => container::run(cli),
        RuntimeMode::Libkrun => libkrun::run(cli),
    }
}

fn resolve_runtime_mode(cli: &Cli) -> Result<RuntimeMode> {
    resolve_runtime_mode_with_sidecar_env(
        cli,
        env_flag_enabled("AGENTBOX_NIX_SIDECAR", DEFAULT_NIX_SIDECAR_ENABLED),
    )
}

fn resolve_runtime_mode_with_sidecar_env(
    cli: &Cli,
    env_sidecar_enabled: Result<bool>,
) -> Result<RuntimeMode> {
    let env_sidecar_disabled = !env_sidecar_enabled?;

    if cli.native && cli.libkrun {
        anyhow::bail!("--native and --libkrun select conflicting runtime modes");
    }

    if cli.libkrun && cli.sidecar_only {
        anyhow::bail!(
            "--sidecar-only is only supported in container mode and cannot be used with --libkrun"
        );
    }

    if cli.libkrun && cli.disable_nix_sidecar {
        anyhow::bail!(
            "--disable-nix-sidecar configures container sidecar mode and cannot be used with --libkrun"
        );
    }

    if cli.libkrun && env_sidecar_disabled {
        anyhow::bail!(
            "AGENTBOX_NIX_SIDECAR=0 configures container sidecar mode and cannot be used with --libkrun"
        );
    }

    let selects_container = cli.native || cli.sidecar_only;

    if selects_container {
        if cli.tsi {
            anyhow::bail!("--tsi is only supported in libkrun mode");
        }
        if cli.mem_gib.is_some() {
            anyhow::bail!("--mem is only supported in libkrun mode");
        }
        if cli.libkrun_debug_entrypoint.is_some() {
            anyhow::bail!("--libkrun-debug-entrypoint is only supported in libkrun mode");
        }
        if cli.libkrun_debug_guest_init.is_some() {
            anyhow::bail!("--libkrun-debug-guest-init is only supported in libkrun mode");
        }
    }

    if !selects_container && (cli.disable_nix_sidecar || env_sidecar_disabled) {
        anyhow::bail!(
            "--disable-nix-sidecar and AGENTBOX_NIX_SIDECAR=0 configure container sidecar mode; libkrun is the default, so pass --native if you intended container mode"
        );
    }

    if !selects_container && cli.profile && cli.debug && cli.libkrun_debug_entrypoint.is_some() {
        anyhow::bail!(
            "--profile --debug cannot be combined with --libkrun-debug-entrypoint because the debug entrypoint bypasses agentbox-guest-init profiling"
        );
    }

    if selects_container {
        return Ok(RuntimeMode::Container);
    }

    Ok(RuntimeMode::Libkrun)
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    fn parse(args: &[&str]) -> Cli {
        let mut argv = vec!["agentbox"];
        argv.extend(args.iter().copied());
        Cli::parse_from(argv)
    }

    fn resolve_with_sidecar_env(args: &[&str], env_sidecar_enabled: bool) -> Result<RuntimeMode> {
        resolve_runtime_mode_with_sidecar_env(&parse(args), Ok(env_sidecar_enabled))
    }

    #[test]
    fn default_mode_is_libkrun() {
        assert_eq!(
            resolve_with_sidecar_env(&[], true).unwrap(),
            RuntimeMode::Libkrun
        );
    }

    #[test]
    fn native_flag_resolves_to_container() {
        assert_eq!(
            resolve_with_sidecar_env(&["--native"], true).unwrap(),
            RuntimeMode::Container
        );
    }

    #[test]
    fn libkrun_flag_resolves_to_libkrun() {
        assert_eq!(
            resolve_with_sidecar_env(&["--libkrun"], true).unwrap(),
            RuntimeMode::Libkrun
        );
    }

    #[test]
    fn native_and_libkrun_conflict() {
        let err = resolve_with_sidecar_env(&["--native", "--libkrun"], true)
            .expect_err("conflicting mode flags should fail");
        assert!(err.to_string().contains("conflicting runtime modes"));
    }

    #[test]
    fn sidecar_only_resolves_to_container() {
        assert_eq!(
            resolve_with_sidecar_env(&["--sidecar-only"], true).unwrap(),
            RuntimeMode::Container
        );
    }

    #[test]
    fn native_sidecar_only_resolves_to_container() {
        assert_eq!(
            resolve_with_sidecar_env(&["--native", "--sidecar-only"], true).unwrap(),
            RuntimeMode::Container
        );
    }

    #[test]
    fn libkrun_and_sidecar_only_conflict() {
        let err = resolve_with_sidecar_env(&["--libkrun", "--sidecar-only"], true)
            .expect_err("--libkrun --sidecar-only should fail");
        let message = err.to_string();
        assert!(message.contains("--libkrun"));
        assert!(message.contains("--sidecar-only"));
    }

    #[test]
    fn disable_sidecar_without_container_selector_rejects_with_native_guidance() {
        let err = resolve_with_sidecar_env(&["--disable-nix-sidecar"], true)
            .expect_err("--disable-nix-sidecar should fail under libkrun default");
        let message = err.to_string();
        assert!(message.contains("--disable-nix-sidecar"));
        assert!(message.contains("AGENTBOX_NIX_SIDECAR=0"));
        assert!(message.contains("--native"));
    }

    #[test]
    fn env_disabled_sidecar_without_container_selector_rejects_with_native_guidance() {
        let err = resolve_with_sidecar_env(&[], false)
            .expect_err("AGENTBOX_NIX_SIDECAR=0 should fail under libkrun default");
        let message = err.to_string();
        assert!(message.contains("AGENTBOX_NIX_SIDECAR=0"));
        assert!(message.contains("--native"));
    }

    #[test]
    fn libkrun_and_disable_sidecar_conflict() {
        let err = resolve_with_sidecar_env(&["--libkrun", "--disable-nix-sidecar"], true)
            .expect_err("--libkrun --disable-nix-sidecar should fail");
        let message = err.to_string();
        assert!(message.contains("--libkrun"));
        assert!(message.contains("--disable-nix-sidecar"));
    }

    #[test]
    fn libkrun_and_env_disabled_sidecar_conflict() {
        let err = resolve_with_sidecar_env(&["--libkrun"], false)
            .expect_err("--libkrun with AGENTBOX_NIX_SIDECAR=0 should fail");
        let message = err.to_string();
        assert!(message.contains("--libkrun"));
        assert!(message.contains("AGENTBOX_NIX_SIDECAR=0"));
    }

    #[test]
    fn native_and_disable_sidecar_resolves_to_container() {
        assert_eq!(
            resolve_with_sidecar_env(&["--native", "--disable-nix-sidecar"], true).unwrap(),
            RuntimeMode::Container
        );
    }

    #[test]
    fn native_and_env_disabled_sidecar_resolves_to_container() {
        assert_eq!(
            resolve_with_sidecar_env(&["--native"], false).unwrap(),
            RuntimeMode::Container
        );
    }

    #[test]
    fn sidecar_only_and_disabled_sidecar_resolves_to_container_for_validation() {
        assert_eq!(
            resolve_with_sidecar_env(&["--sidecar-only", "--disable-nix-sidecar"], true).unwrap(),
            RuntimeMode::Container
        );
    }

    #[test]
    fn sidecar_only_and_env_disabled_sidecar_resolves_to_container_for_validation() {
        assert_eq!(
            resolve_with_sidecar_env(&["--sidecar-only"], false).unwrap(),
            RuntimeMode::Container
        );
    }

    #[test]
    fn env_parse_error_is_preserved_before_runtime_selection() {
        let err = resolve_runtime_mode_with_sidecar_env(
            &parse(&[]),
            Err(anyhow::anyhow!(
                "environment variable 'AGENTBOX_NIX_SIDECAR' must be one of: 1, true, yes, on, 0, false, no, off"
            )),
        )
        .expect_err("invalid env should fail before runtime selection");
        assert!(err
            .to_string()
            .contains("environment variable 'AGENTBOX_NIX_SIDECAR' must be one of"));
    }

    #[test]
    fn libkrun_options_are_valid_in_default_mode() {
        assert_eq!(
            resolve_with_sidecar_env(&["--tsi"], true).unwrap(),
            RuntimeMode::Libkrun
        );
        assert_eq!(
            resolve_with_sidecar_env(&["--mem", "8"], true).unwrap(),
            RuntimeMode::Libkrun
        );
        assert_eq!(
            resolve_with_sidecar_env(
                &["--libkrun-debug-entrypoint", "./debug-entrypoint.sh",],
                true,
            )
            .unwrap(),
            RuntimeMode::Libkrun
        );
        assert_eq!(
            resolve_with_sidecar_env(
                &["--libkrun-debug-guest-init", "./agentbox-guest-init",],
                true,
            )
            .unwrap(),
            RuntimeMode::Libkrun
        );
    }

    #[test]
    fn libkrun_options_conflict_with_native_mode() {
        let tsi_err = resolve_with_sidecar_env(&["--native", "--tsi"], true)
            .expect_err("--native --tsi should fail");
        assert!(tsi_err
            .to_string()
            .contains("--tsi is only supported in libkrun mode"));

        let mem_err = resolve_with_sidecar_env(&["--native", "--mem", "8"], true)
            .expect_err("--native --mem should fail");
        assert!(mem_err
            .to_string()
            .contains("--mem is only supported in libkrun mode"));

        let debug_err = resolve_with_sidecar_env(
            &[
                "--native",
                "--libkrun-debug-entrypoint",
                "./debug-entrypoint.sh",
            ],
            true,
        )
        .expect_err("--native --libkrun-debug-entrypoint should fail");
        assert!(debug_err
            .to_string()
            .contains("--libkrun-debug-entrypoint is only supported in libkrun mode"));

        let guest_init_err = resolve_with_sidecar_env(
            &[
                "--native",
                "--libkrun-debug-guest-init",
                "./agentbox-guest-init",
            ],
            true,
        )
        .expect_err("--native --libkrun-debug-guest-init should fail");
        assert!(guest_init_err
            .to_string()
            .contains("--libkrun-debug-guest-init is only supported in libkrun mode"));
    }

    #[test]
    fn libkrun_options_conflict_with_sidecar_only_mode() {
        let tsi_err = resolve_with_sidecar_env(&["--sidecar-only", "--tsi"], true)
            .expect_err("--sidecar-only --tsi should fail");
        assert!(tsi_err
            .to_string()
            .contains("--tsi is only supported in libkrun mode"));

        let mem_err = resolve_with_sidecar_env(&["--sidecar-only", "--mem", "8"], true)
            .expect_err("--sidecar-only --mem should fail");
        assert!(mem_err
            .to_string()
            .contains("--mem is only supported in libkrun mode"));

        let debug_err = resolve_with_sidecar_env(
            &[
                "--sidecar-only",
                "--libkrun-debug-entrypoint",
                "./debug-entrypoint.sh",
            ],
            true,
        )
        .expect_err("--sidecar-only --libkrun-debug-entrypoint should fail");
        assert!(debug_err
            .to_string()
            .contains("--libkrun-debug-entrypoint is only supported in libkrun mode"));

        let guest_init_err = resolve_with_sidecar_env(
            &[
                "--sidecar-only",
                "--libkrun-debug-guest-init",
                "./agentbox-guest-init",
            ],
            true,
        )
        .expect_err("--sidecar-only --libkrun-debug-guest-init should fail");
        assert!(guest_init_err
            .to_string()
            .contains("--libkrun-debug-guest-init is only supported in libkrun mode"));
    }

    #[test]
    fn debug_entrypoint_flag_is_valid_with_explicit_libkrun() {
        assert_eq!(
            resolve_with_sidecar_env(
                &[
                    "--libkrun",
                    "--libkrun-debug-entrypoint",
                    "./debug-entrypoint.sh",
                ],
                true,
            )
            .unwrap(),
            RuntimeMode::Libkrun
        );
    }

    #[test]
    fn debug_guest_init_flag_is_valid_with_explicit_libkrun() {
        assert_eq!(
            resolve_with_sidecar_env(
                &[
                    "--libkrun",
                    "--libkrun-debug-guest-init",
                    "./agentbox-guest-init",
                ],
                true,
            )
            .unwrap(),
            RuntimeMode::Libkrun
        );
    }

    #[test]
    fn native_and_debug_entrypoint_conflict() {
        let err = resolve_with_sidecar_env(
            &[
                "--native",
                "--libkrun-debug-entrypoint",
                "./debug-entrypoint.sh",
            ],
            true,
        )
        .expect_err("--libkrun-debug-entrypoint should fail in container mode");
        assert!(err
            .to_string()
            .contains("--libkrun-debug-entrypoint is only supported in libkrun mode"));
    }

    #[test]
    fn profile_debug_rejects_libkrun_debug_entrypoint_bypass() {
        let err = resolve_with_sidecar_env(
            &[
                "--profile",
                "--debug",
                "--libkrun-debug-entrypoint",
                "./debug-entrypoint.sh",
            ],
            true,
        )
        .expect_err("--profile --debug with debug entrypoint should fail");

        let message = err.to_string();
        assert!(message.contains("--profile --debug"));
        assert!(message.contains("--libkrun-debug-entrypoint"));
        assert!(message.contains("bypasses agentbox-guest-init profiling"));
    }
}
