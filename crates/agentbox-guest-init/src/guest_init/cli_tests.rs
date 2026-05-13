use clap::Parser;

use crate::guest_init::cli::{
    ContainerSubcommand, GuestInitCli, LibkrunSubcommand, PodmanSubcommand, RuntimeCommand,
};

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
fn parses_container_enter_shape_without_wiring_image_to_it() {
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
