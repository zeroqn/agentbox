use super::*;
use std::path::Path;

#[test]
fn unshare_cleanup_runs_inside_podman_unshare() {
    let args = build_unshare_cleanup_args(Path::new("/tmp/agentbox/nix-merged"));

    assert_eq!(args[0], "unshare");
    assert_eq!(args[1], "bash");
    assert_eq!(args[2], "-lc");
    assert!(args[3].contains("/proc/self/mountinfo"));
    assert!(args[3].contains("fusermount3 -u"));
    assert_eq!(args[4], "agentbox");
    assert_eq!(args[5], "/tmp/agentbox/nix-merged");
}

#[test]
fn unshare_cleanup_script_succeeds_when_mount_is_absent() {
    let absent_mount = "/tmp/agentbox-nix-merged-not-mounted";
    let status = Command::new("bash")
        .arg("-lc")
        .arg(UNSHARE_CLEANUP_SCRIPT)
        .arg("agentbox")
        .arg(absent_mount)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("bash should run cleanup script");

    assert!(status.success());
}

#[test]
fn direct_cleanup_commands_do_not_enter_podman_unshare() {
    let commands = current_namespace_unmount_commands();

    assert_eq!(commands[0], ("fusermount3", vec!["-u"]));
    assert_eq!(commands[1], ("fusermount", vec!["-u"]));
    assert_eq!(commands[2], ("umount", vec![]));
}
