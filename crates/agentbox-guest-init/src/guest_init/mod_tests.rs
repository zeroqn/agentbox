use crate::guest_init::components::nix::root::{planned_operations as nix_ops, NixOperation};
use crate::guest_init::components::podman::root::{
    planned_operations as podman_ops, PodmanPrepOperation,
};

#[test]
fn orchestration_derives_shell_env_before_background_podman_prep() {
    let events = [
        "parse-env",
        "materialize-home",
        "derive-parent-shell-env",
        "start-background-nix-prep",
        "start-background-podman-prep",
        "drop-and-exec",
    ];
    let pos = |name| events.iter().position(|event| event == &name).unwrap();
    assert!(pos("derive-parent-shell-env") < pos("start-background-podman-prep"));
    assert!(pos("start-background-nix-prep") < pos("start-background-podman-prep"));
    assert!(pos("start-background-nix-prep") < pos("drop-and-exec"));
}

#[test]
fn nix_and_podman_root_prep_are_lazy() {
    let nix = nix_ops();
    assert_eq!(nix.first(), Some(&NixOperation::WriteRunningStatus));
    assert_eq!(nix.last(), Some(&NixOperation::WriteReadyStatus));
    let podman = podman_ops();
    assert_eq!(
        podman.first(),
        Some(&PodmanPrepOperation::WriteRunningStatus)
    );
}
