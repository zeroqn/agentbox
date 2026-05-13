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
        "bootstrap-nix",
        "start-background-podman-prep",
        "drop-and-exec",
    ];
    let pos = |name| events.iter().position(|event| event == &name).unwrap();
    assert!(pos("derive-parent-shell-env") < pos("start-background-podman-prep"));
    assert!(pos("bootstrap-nix") < pos("drop-and-exec"));
}

#[test]
fn nix_remains_blocking_but_podman_prep_is_lazy() {
    let nix = nix_ops();
    assert!(nix.contains(&NixOperation::WaitSocket));
    let podman = podman_ops();
    assert_eq!(
        podman.first(),
        Some(&PodmanPrepOperation::WriteRunningStatus)
    );
}
