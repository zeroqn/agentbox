use anyhow::{anyhow, Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

const APP_DIR_NAME: &str = "agentbox";
const CONFIG_FILE_NAME: &str = "agentbox.toml";
const STATE_CONFIG_SECTION: &str = "state";
const STATE_LOCATION_KEY: &str = "location";

pub fn read_state_location_override(
    xdg_config_home: Option<&Path>,
    home_dir: Option<&Path>,
) -> Result<Option<PathBuf>> {
    let config_path = default_config_path(xdg_config_home, home_dir)?;
    read_state_location_override_from_path(&config_path)
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

fn read_state_location_override_from_path(config_path: &Path) -> Result<Option<PathBuf>> {
    if !config_path.exists() {
        return Ok(None);
    }

    let contents = fs::read_to_string(config_path)
        .with_context(|| format!("failed to read '{}'", config_path.display()))?;
    parse_state_location_override(&contents).with_context(|| {
        format!(
            "failed to parse state location from '{}'",
            config_path.display()
        )
    })
}

fn parse_state_location_override(contents: &str) -> Result<Option<PathBuf>> {
    let mut in_state_section = false;

    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';') {
            continue;
        }

        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_state_section = trimmed[1..trimmed.len() - 1].trim() == STATE_CONFIG_SECTION;
            continue;
        }

        if !in_state_section {
            continue;
        }

        let Some((key, value)) = trimmed.split_once('=') else {
            continue;
        };

        if key.trim() != STATE_LOCATION_KEY {
            continue;
        }

        let value = value.trim();
        if value.len() < 2 || !value.starts_with('"') || !value.ends_with('"') {
            return Err(anyhow!(
                "[state].location must be a double-quoted absolute path"
            ));
        }

        let path = PathBuf::from(&value[1..value.len() - 1]);
        if !path.is_absolute() {
            return Err(anyhow!("[state].location must be an absolute path"));
        }

        return Ok(Some(path));
    }

    Ok(None)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    #[test]
    fn default_config_path_prefers_xdg_config_home() {
        let path = crate::config::state::default_config_path(
            Some(Path::new("/tmp/xdg-config")),
            Some(Path::new("/tmp/home")),
        )
        .expect("xdg config path should resolve");

        assert_eq!(path, Path::new("/tmp/xdg-config/agentbox/agentbox.toml"));
    }

    #[test]
    fn default_config_path_falls_back_to_home_config() {
        let path = crate::config::state::default_config_path(None, Some(Path::new("/tmp/home")))
            .expect("fallback should work");

        assert_eq!(path, Path::new("/tmp/home/.config/agentbox/agentbox.toml"));
    }

    #[test]
    fn parse_state_location_override_accepts_absolute_path() {
        let path = crate::config::state::parse_state_location_override(
            "[state]\nlocation = \"/tmp/custom/\"\n",
        )
        .expect("config should parse")
        .expect("location should exist");

        assert_eq!(path, Path::new("/tmp/custom/"));
    }

    #[test]
    fn parse_state_location_override_rejects_relative_path() {
        let err = crate::config::state::parse_state_location_override(
            "[state]\nlocation = \"relative/path\"\n",
        )
        .expect_err("relative path should fail");

        assert!(err.to_string().contains("absolute path"));
    }
}
