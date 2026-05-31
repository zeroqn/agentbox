use anyhow::{Context, Result, anyhow};
use std::fs;
use std::path::{Path, PathBuf};

use crate::task_rootfs::TaskRootfsBackend;

const APP_DIR_NAME: &str = "loftd";
const CONFIG_FILE_NAME: &str = "loftd.toml";
const STATE_CONFIG_SECTION: &str = "state";
const STATE_LOCATION_KEY: &str = "location";
const TASK_ROOTFS_CONFIG_SECTION: &str = "task-rootfs";
const TASK_ROOTFS_BACKEND_KEY: &str = "backend";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LoftdConfig {
    path: PathBuf,
    loaded: bool,
    state_location_override: Option<PathBuf>,
    task_rootfs_backend: Option<TaskRootfsBackend>,
}

impl LoftdConfig {
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn loaded(&self) -> bool {
        self.loaded
    }

    pub(crate) fn state_location_override(&self) -> Option<&Path> {
        self.state_location_override.as_deref()
    }

    pub(crate) fn task_rootfs_backend(&self) -> Option<TaskRootfsBackend> {
        self.task_rootfs_backend
    }

    fn missing(path: PathBuf) -> Self {
        Self {
            path,
            loaded: false,
            state_location_override: None,
            task_rootfs_backend: None,
        }
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct ParsedConfig {
    state_location_override: Option<PathBuf>,
    task_rootfs_backend: Option<TaskRootfsBackend>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Section {
    State,
    TaskRootfs,
    Unknown,
}

pub(crate) fn read_config(
    xdg_config_home: Option<&Path>,
    home_dir: Option<&Path>,
) -> Result<LoftdConfig> {
    let config_path = default_config_path(xdg_config_home, home_dir)?;
    read_config_from_path(&config_path)
}

fn default_config_path(xdg_config_home: Option<&Path>, home_dir: Option<&Path>) -> Result<PathBuf> {
    if let Some(path) = xdg_config_home {
        return Ok(path.join(APP_DIR_NAME).join(CONFIG_FILE_NAME));
    }

    let home_dir =
        home_dir.ok_or_else(|| anyhow!("HOME is not set and XDG_CONFIG_HOME is not available"))?;
    Ok(home_dir
        .join(".config")
        .join(APP_DIR_NAME)
        .join(CONFIG_FILE_NAME))
}

fn read_config_from_path(config_path: &Path) -> Result<LoftdConfig> {
    if !config_path.exists() {
        return Ok(LoftdConfig::missing(config_path.to_path_buf()));
    }

    let contents = fs::read_to_string(config_path)
        .with_context(|| format!("failed to read '{}'", config_path.display()))?;
    let parsed = parse_config(&contents)
        .with_context(|| format!("failed to parse '{}'", config_path.display()))?;

    Ok(LoftdConfig {
        path: config_path.to_path_buf(),
        loaded: true,
        state_location_override: parsed.state_location_override,
        task_rootfs_backend: parsed.task_rootfs_backend,
    })
}

fn parse_config(contents: &str) -> Result<ParsedConfig> {
    let mut section = Section::Unknown;
    let mut parsed = ParsedConfig::default();

    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';') {
            continue;
        }

        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            section = match trimmed[1..trimmed.len() - 1].trim() {
                STATE_CONFIG_SECTION => Section::State,
                TASK_ROOTFS_CONFIG_SECTION => Section::TaskRootfs,
                _ => Section::Unknown,
            };
            continue;
        }

        let Some((key, value)) = trimmed.split_once('=') else {
            continue;
        };
        let key = key.trim();

        match section {
            Section::State if key == STATE_LOCATION_KEY => {
                parsed.state_location_override = Some(parse_state_location_value(value.trim())?);
            }
            Section::TaskRootfs if key == TASK_ROOTFS_BACKEND_KEY => {
                parsed.task_rootfs_backend = Some(parse_task_rootfs_backend_value(value.trim())?);
            }
            _ => {}
        }
    }

    Ok(parsed)
}

fn parse_state_location_value(value: &str) -> Result<PathBuf> {
    let value = parse_double_quoted_string(value)
        .ok_or_else(|| anyhow!("[state].location must be a double-quoted absolute path"))?;

    let path = PathBuf::from(value);
    if !path.is_absolute() {
        return Err(anyhow!("[state].location must be an absolute path"));
    }

    Ok(path)
}

fn parse_task_rootfs_backend_value(value: &str) -> Result<TaskRootfsBackend> {
    let value = parse_double_quoted_string(value)
        .ok_or_else(|| anyhow!("[task-rootfs].backend must be a double-quoted string"))?;

    TaskRootfsBackend::parse_config_value(value).map_err(|err| anyhow!(err))
}

fn parse_double_quoted_string(value: &str) -> Option<&str> {
    if value.len() < 2 || !value.starts_with('"') || !value.ends_with('"') {
        return None;
    }

    Some(&value[1..value.len() - 1])
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::task_rootfs::TaskRootfsBackend;

    #[test]
    fn default_config_path_prefers_xdg_config_home() {
        let path = crate::config::state::default_config_path(
            Some(Path::new("/tmp/xdg-config")),
            Some(Path::new("/tmp/home")),
        )
        .expect("xdg config path should resolve");

        assert_eq!(path, Path::new("/tmp/xdg-config/loftd/loftd.toml"));
    }

    #[test]
    fn default_config_path_falls_back_to_home_config() {
        let path = crate::config::state::default_config_path(None, Some(Path::new("/tmp/home")))
            .expect("fallback should work");

        assert_eq!(path, Path::new("/tmp/home/.config/loftd/loftd.toml"));
    }

    #[test]
    fn read_config_reports_missing_file_diagnostics() {
        let dir = tempfile::tempdir().expect("tempdir should exist");
        let config = crate::config::state::read_config(Some(dir.path()), None)
            .expect("missing config should still resolve");

        assert_eq!(config.path(), &dir.path().join("loftd").join("loftd.toml"));
        assert!(!config.loaded());
        assert_eq!(config.state_location_override(), None);
        assert_eq!(config.task_rootfs_backend(), None);
    }

    #[test]
    fn parse_config_accepts_state_location_and_task_rootfs_backend() {
        let config = crate::config::state::parse_config(
            "[state]\nlocation = \"/tmp/custom/\"\n\n[task-rootfs]\nbackend = \"fuse-overlay\"\n",
        )
        .expect("config should parse");

        assert_eq!(
            config.state_location_override.as_deref(),
            Some(Path::new("/tmp/custom/"))
        );
        assert_eq!(
            config.task_rootfs_backend,
            Some(TaskRootfsBackend::FuseOverlay)
        );
    }

    #[test]
    fn parse_config_rejects_relative_state_location() {
        let err = crate::config::state::parse_config("[state]\nlocation = \"relative/path\"\n")
            .expect_err("relative path should fail");

        assert!(err.to_string().contains("absolute path"));
    }

    #[test]
    fn parse_config_rejects_invalid_task_rootfs_backend() {
        let err = crate::config::state::parse_config("[task-rootfs]\nbackend = \"auto\"\n")
            .expect_err("auto backend should fail");

        assert!(err.to_string().contains("task rootfs backend"));
    }

    #[test]
    fn parse_config_rejects_malformed_known_key_values() {
        let state_err = crate::config::state::parse_config("[state]\nlocation = /tmp/custom\n")
            .expect_err("unquoted state path should fail");
        let backend_err =
            crate::config::state::parse_config("[task-rootfs]\nbackend = fuse-overlay\n")
                .expect_err("unquoted backend should fail");

        assert!(state_err.to_string().contains("double-quoted"));
        assert!(backend_err.to_string().contains("double-quoted"));
    }

    #[test]
    fn parser_ignores_unknown_sections_and_keys() {
        let config = crate::config::state::parse_config(
            "[agentbox]\nlocation = \"/tmp/wrong\"\n[task-rootfs]\nunknown = \"auto\"\n",
        )
        .expect("unknown config should be ignored");

        assert_eq!(config.state_location_override, None);
        assert_eq!(config.task_rootfs_backend, None);
    }
}
