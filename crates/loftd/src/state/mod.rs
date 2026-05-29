use anyhow::{Result, anyhow};
use std::env;
use std::path::{Path, PathBuf};

use crate::config;
use crate::naming::derive_workspace_slug;

const APP_DIR_NAME: &str = "loftd";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StateLayout {
    app_dir: PathBuf,
    root_dir: PathBuf,
}

impl StateLayout {
    pub(crate) fn root_dir(&self) -> &Path {
        &self.root_dir
    }

    pub(crate) fn image_cache_dir(&self) -> PathBuf {
        self.app_dir.join("microvm").join("images")
    }

    pub(crate) fn sccache_dir(&self) -> PathBuf {
        self.app_dir.join("sccache")
    }

    fn new(app_dir: PathBuf, root_dir: PathBuf) -> Self {
        Self { app_dir, root_dir }
    }
}

pub(crate) fn resolve_state_layout(cwd: &Path) -> Result<StateLayout> {
    let xdg_state_home = env::var_os("XDG_STATE_HOME").map(PathBuf::from);
    let xdg_config_home = env::var_os("XDG_CONFIG_HOME").map(PathBuf::from);
    let home_dir = env::var_os("HOME").map(PathBuf::from);

    resolve_state_layout_from_env(
        cwd,
        xdg_state_home.as_deref(),
        xdg_config_home.as_deref(),
        home_dir.as_deref(),
    )
}

fn resolve_state_layout_from_env(
    cwd: &Path,
    xdg_state_home: Option<&Path>,
    xdg_config_home: Option<&Path>,
    home_dir: Option<&Path>,
) -> Result<StateLayout> {
    let default_location_root = default_state_location_root(xdg_state_home, home_dir)?;
    let location_root = config::state::read_state_location_override(xdg_config_home, home_dir)?
        .unwrap_or(default_location_root);

    let app_dir = location_root.join(APP_DIR_NAME);
    Ok(StateLayout::new(
        app_dir.clone(),
        app_dir.join(derive_workspace_slug(cwd)),
    ))
}

fn default_state_location_root(
    xdg_state_home: Option<&Path>,
    home_dir: Option<&Path>,
) -> Result<PathBuf> {
    if let Some(path) = xdg_state_home {
        return Ok(path.to_path_buf());
    }

    let home_dir =
        home_dir.ok_or_else(|| anyhow!("HOME is not set and XDG_STATE_HOME is not available"))?;
    Ok(home_dir.join(".local").join("state"))
}

#[cfg(test)]
mod tests {
    use crate::state::{default_state_location_root, resolve_state_layout_from_env};
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
    fn resolve_state_layout_uses_default_xdg_state_root() {
        let layout = resolve_state_layout_from_env(
            Path::new("/tmp/project"),
            Some(Path::new("/tmp/xdg-state")),
            Some(Path::new("/tmp/xdg-config")),
            Some(Path::new("/tmp/home")),
        )
        .expect("layout should resolve");

        assert_eq!(layout.root_dir(), Path::new("/tmp/xdg-state/loftd/project"));
        assert_eq!(
            layout.sccache_dir(),
            Path::new("/tmp/xdg-state/loftd/sccache")
        );
        assert_eq!(
            layout.image_cache_dir(),
            Path::new("/tmp/xdg-state/loftd/microvm/images")
        );
        assert!(!layout.image_cache_dir().starts_with(layout.root_dir()));
    }

    #[test]
    fn resolve_state_layout_honors_config_override_and_appends_loftd() {
        let dir = tempfile::tempdir().expect("tempdir should be created");
        let config_home = dir.path().join("config");
        let state_home = dir.path().join("state");
        let home = dir.path().join("home");

        fs::create_dir_all(config_home.join("loftd")).expect("config dir should exist");
        fs::write(
            config_home.join("loftd").join("loftd.toml"),
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
            layout.root_dir(),
            Path::new("/tmp/custom-root/loftd/project")
        );
        assert_eq!(
            layout.sccache_dir(),
            Path::new("/tmp/custom-root/loftd/sccache")
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

        assert_eq!(layout.root_dir(), &state_home.join("loftd").join("project"));
        assert_ne!(layout.root_dir(), workspace.join(".agentbox"));
    }
}
