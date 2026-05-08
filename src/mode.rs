use anyhow::Result;

use crate::cli::Cli;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RuntimeMode {
    Container,
    Libkrun,
}

pub(crate) fn resolve_runtime_mode(cli: &Cli) -> Result<RuntimeMode> {
    if cli.native && cli.libkrun {
        anyhow::bail!("--native and --libkrun select conflicting runtime modes");
    }

    if !cli.libkrun {
        if cli.tsi {
            anyhow::bail!("--tsi is only supported with --libkrun");
        }
        if cli.mem_gib.is_some() {
            anyhow::bail!("--mem is only supported with --libkrun");
        }
    }

    if cli.libkrun {
        Ok(RuntimeMode::Libkrun)
    } else {
        Ok(RuntimeMode::Container)
    }
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

    #[test]
    fn default_mode_is_container() {
        assert_eq!(
            resolve_runtime_mode(&parse(&[])).unwrap(),
            RuntimeMode::Container
        );
    }

    #[test]
    fn native_alias_resolves_to_container() {
        assert_eq!(
            resolve_runtime_mode(&parse(&["--native"])).unwrap(),
            RuntimeMode::Container
        );
    }

    #[test]
    fn libkrun_opt_in_resolves_to_libkrun() {
        assert_eq!(
            resolve_runtime_mode(&parse(&["--libkrun"])).unwrap(),
            RuntimeMode::Libkrun
        );
    }

    #[test]
    fn native_and_libkrun_conflict() {
        let err = resolve_runtime_mode(&parse(&["--native", "--libkrun"]))
            .expect_err("conflicting mode flags should fail");
        assert!(err.to_string().contains("conflicting runtime modes"));
    }

    #[test]
    fn tsi_requires_libkrun() {
        let err = resolve_runtime_mode(&parse(&["--tsi"])).expect_err("--tsi should fail");
        assert!(err
            .to_string()
            .contains("--tsi is only supported with --libkrun"));
    }

    #[test]
    fn mem_requires_libkrun() {
        let err = resolve_runtime_mode(&parse(&["--mem", "8"])).expect_err("--mem should fail");
        assert!(err
            .to_string()
            .contains("--mem is only supported with --libkrun"));
    }
}
