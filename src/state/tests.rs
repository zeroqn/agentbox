use super::*;
use std::fs;
use std::path::Path;

#[test]
fn default_state_location_root_prefers_xdg_state_home() {
    let path = default_state_location_root(
        Some(Path::new("/tmp/xdg-state")),
        Some(Path::new("/tmp/home")),
    )
    .expect("xdg state home should resolve");

    assert_eq!(path, Path::new("/tmp/xdg-state"));
}

#[test]
fn default_state_location_root_falls_back_to_home_local_state() {
    let path = default_state_location_root(None, Some(Path::new("/tmp/home")))
        .expect("fallback should work");

    assert_eq!(path, Path::new("/tmp/home/.local/state"));
}

#[test]
fn default_config_path_prefers_xdg_config_home() {
    let path = default_config_path(
        Some(Path::new("/tmp/xdg-config")),
        Some(Path::new("/tmp/home")),
    )
    .expect("xdg config path should resolve");

    assert_eq!(path, Path::new("/tmp/xdg-config/agentbox/agentbox.toml"));
}

#[test]
fn default_config_path_falls_back_to_home_config() {
    let path =
        default_config_path(None, Some(Path::new("/tmp/home"))).expect("fallback should work");

    assert_eq!(path, Path::new("/tmp/home/.config/agentbox/agentbox.toml"));
}

#[test]
fn parse_state_location_override_accepts_absolute_path() {
    let path = parse_state_location_override("[state]\nlocation = \"/tmp/custom/\"\n")
        .expect("config should parse")
        .expect("location should exist");

    assert_eq!(path, Path::new("/tmp/custom/"));
}

#[test]
fn parse_state_location_override_rejects_relative_path() {
    let err = parse_state_location_override("[state]\nlocation = \"relative/path\"\n")
        .expect_err("relative path should fail");

    assert!(err.to_string().contains("absolute path"));
}

#[test]
fn resolve_state_layout_uses_default_xdg_state_root() {
    let layout = resolve_state_layout_from_env(
        Path::new("/tmp/project"),
        Some(Path::new("/tmp/xdg-state")),
        Some(Path::new("/tmp/xdg-config")),
        Some(Path::new("/tmp/home")),
    )
    .expect("layout should resolve");

    assert_eq!(
        layout.root_dir,
        Path::new("/tmp/xdg-state/agentbox/project")
    );
    assert_eq!(
        layout.root_dir.join("cargo"),
        Path::new("/tmp/xdg-state/agentbox/project/cargo")
    );
}

#[test]
fn resolve_state_layout_honors_config_override_and_appends_agentbox() {
    let dir = tempfile::tempdir().expect("tempdir should be created");
    let config_home = dir.path().join("config");
    let state_home = dir.path().join("state");
    let home = dir.path().join("home");

    fs::create_dir_all(config_home.join("agentbox")).expect("config dir should exist");
    fs::write(
        config_home.join("agentbox").join("agentbox.toml"),
        "[state]\nlocation = \"/tmp/custom-root/\"\n",
    )
    .expect("config file should be written");

    let layout = resolve_state_layout_from_env(
        Path::new("/tmp/project"),
        Some(&state_home),
        Some(&config_home),
        Some(&home),
    )
    .expect("layout should resolve");

    assert_eq!(
        layout.root_dir,
        Path::new("/tmp/custom-root/agentbox/project")
    );
}

#[test]
fn resolve_state_layout_ignores_legacy_repo_local_agentbox() {
    let dir = tempfile::tempdir().expect("tempdir should be created");
    let workspace = dir.path().join("project");
    let state_home = dir.path().join("state");
    let config_home = dir.path().join("config");
    let home = dir.path().join("home");

    fs::create_dir_all(workspace.join(".agentbox").join("nix"))
        .expect("legacy state should be created");

    let layout = resolve_state_layout_from_env(
        &workspace,
        Some(&state_home),
        Some(&config_home),
        Some(&home),
    )
    .expect("layout should resolve");

    assert_eq!(layout.root_dir, state_home.join("agentbox").join("project"));
    assert_ne!(layout.root_dir, workspace.join(".agentbox"));
}
