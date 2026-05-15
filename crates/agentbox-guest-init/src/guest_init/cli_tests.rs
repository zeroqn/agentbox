use clap::Parser;

use crate::guest_init::cli::{
    ContainerSubcommand, DefaultSubcommand, GuestInitCli, LibkrunSubcommand, NixSubcommand,
    PodmanSubcommand, RuntimeCommand,
};

#[test]
fn parses_default_enter_command_for_image_entrypoint() {
    let cli = GuestInitCli::try_parse_from([
        "agentbox-guest-init",
        "default",
        "enter",
        "--",
        "bash",
        "-lc",
        "true",
    ])
    .unwrap();

    let RuntimeCommand::Default(default) = cli.runtime else {
        panic!("expected default command");
    };
    let DefaultSubcommand::Enter(enter) = default.command;
    assert_eq!(enter.command, ["bash", "-lc", "true"]);
}

#[test]
fn parses_default_enter_command_defaulting_to_login_shell() {
    let cli = GuestInitCli::try_parse_from(["agentbox-guest-init", "default", "enter"]).unwrap();

    let RuntimeCommand::Default(default) = cli.runtime else {
        panic!("expected default command");
    };
    let DefaultSubcommand::Enter(enter) = default.command;
    assert_eq!(enter.resolved_command(), ["fish", "-l"]);
}

#[test]
fn parses_libkrun_enter_command_after_separator() {
    let cli = GuestInitCli::try_parse_from([
        "agentbox-guest-init",
        "libkrun",
        "enter",
        "--",
        "fish",
        "-l",
    ])
    .unwrap();

    let RuntimeCommand::Libkrun(libkrun) = cli.runtime else {
        panic!("expected libkrun command");
    };
    let LibkrunSubcommand::Enter(enter) = libkrun.command else {
        panic!("expected enter command");
    };
    assert_eq!(enter.command, ["fish", "-l"]);
}

#[test]
fn parses_default_libkrun_enter_command() {
    let cli = GuestInitCli::try_parse_from(["agentbox-guest-init", "libkrun", "enter"]).unwrap();

    let RuntimeCommand::Libkrun(libkrun) = cli.runtime else {
        panic!("expected libkrun command");
    };
    let LibkrunSubcommand::Enter(enter) = libkrun.command else {
        panic!("expected enter command");
    };
    assert_eq!(enter.resolved_command(), ["fish", "-l"]);
}

#[test]
fn parses_libkrun_podman_prep_and_wait() {
    for (arg, expected) in [
        ("prep", PodmanSubcommand::Prep),
        ("wait", PodmanSubcommand::Wait),
    ] {
        let cli = GuestInitCli::try_parse_from(["agentbox-guest-init", "libkrun", "podman", arg])
            .unwrap();
        let RuntimeCommand::Libkrun(libkrun) = cli.runtime else {
            panic!("expected libkrun command");
        };
        let LibkrunSubcommand::Podman(podman) = libkrun.command else {
            panic!("expected podman command");
        };
        assert_eq!(podman.command, expected);
    }
}

#[test]
fn parses_libkrun_nix_prep_and_wait() {
    for (arg, expected) in [("prep", NixSubcommand::Prep), ("wait", NixSubcommand::Wait)] {
        let cli =
            GuestInitCli::try_parse_from(["agentbox-guest-init", "libkrun", "nix", arg]).unwrap();
        let RuntimeCommand::Libkrun(libkrun) = cli.runtime else {
            panic!("expected libkrun command");
        };
        let LibkrunSubcommand::Nix(nix) = libkrun.command else {
            panic!("expected nix command");
        };
        assert_eq!(nix.command, expected);
    }
}

#[test]
fn parses_container_enter_command() {
    let cli = GuestInitCli::try_parse_from([
        "agentbox-guest-init",
        "container",
        "enter",
        "--",
        "bash",
        "-lc",
        "true",
    ])
    .unwrap();

    let RuntimeCommand::Container(container) = cli.runtime else {
        panic!("expected container command");
    };
    let ContainerSubcommand::Enter(enter) = container.command;
    assert_eq!(enter.command, ["bash", "-lc", "true"]);
}
