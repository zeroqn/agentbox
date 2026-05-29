use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser, PartialEq, Eq)]
#[command(
    name = "loftd-guest-init",
    version,
    about = "Initialize a loftd direct-libkrun microvm guest"
)]
pub(in crate::guest_init) struct GuestInitCli {
    #[command(subcommand)]
    pub(in crate::guest_init) command: GuestInitCommand,
}

#[derive(Debug, Subcommand, PartialEq, Eq)]
pub(in crate::guest_init) enum GuestInitCommand {
    Enter(EnterCommand),
}

#[derive(Debug, Args, Clone, PartialEq, Eq)]
pub(in crate::guest_init) struct EnterCommand {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub(in crate::guest_init) command: Vec<String>,
}

impl EnterCommand {
    pub(in crate::guest_init) fn resolved_command(&self) -> Vec<String> {
        if self.command.is_empty() {
            vec!["fish".to_owned(), "-l".to_owned()]
        } else {
            self.command.clone()
        }
    }
}

#[cfg(test)]
mod tests {
    use clap::{CommandFactory, Parser};

    use crate::guest_init::cli::{GuestInitCli, GuestInitCommand};

    #[test]
    fn command_help_renders() {
        GuestInitCli::command().debug_assert();
    }

    #[test]
    fn parses_enter_command() {
        let cli =
            GuestInitCli::try_parse_from(["loftd-guest-init", "enter", "bash", "-lc", "echo ok"])
                .expect("enter command should parse");

        let GuestInitCommand::Enter(command) = cli.command;
        assert_eq!(command.resolved_command(), ["bash", "-lc", "echo ok"]);
    }

    #[test]
    fn enter_defaults_to_login_fish() {
        let cli = GuestInitCli::try_parse_from(["loftd-guest-init", "enter"])
            .expect("enter command should parse");

        let GuestInitCommand::Enter(command) = cli.command;
        assert_eq!(command.resolved_command(), ["fish", "-l"]);
    }

    #[test]
    fn does_not_accept_legacy_agentbox_runtime_surface() {
        let err = GuestInitCli::try_parse_from(["loftd-guest-init", "microvm", "enter"])
            .expect_err("legacy runtime names should not parse");

        assert_eq!(err.kind(), clap::error::ErrorKind::InvalidSubcommand);
    }
}
