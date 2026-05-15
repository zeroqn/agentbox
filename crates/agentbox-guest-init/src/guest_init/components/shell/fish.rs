use anyhow::{Context, Result, bail};
use std::env;
use std::path::{Path, PathBuf};

use crate::guest_init::components::home::identity::DevIdentity;
use crate::guest_init::components::shell::starship;
use crate::guest_init::fs;

const FISH_CONFIG_SOURCE_ENV: &str = "AGENTBOX_FISH_CONFIG_SOURCE";
const STARSHIP_CONFIG_SOURCE_ENV: &str = "AGENTBOX_STARSHIP_CONFIG_SOURCE";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::guest_init) struct ShellConfigSources {
    pub(in crate::guest_init) fish_config: PathBuf,
    pub(in crate::guest_init) starship_config: PathBuf,
}

impl ShellConfigSources {
    fn from_env() -> Result<Option<Self>> {
        match (
            env::var_os(FISH_CONFIG_SOURCE_ENV),
            env::var_os(STARSHIP_CONFIG_SOURCE_ENV),
        ) {
            (Some(fish_config), Some(starship_config)) => Ok(Some(Self {
                fish_config: PathBuf::from(fish_config),
                starship_config: PathBuf::from(starship_config),
            })),
            (None, None) => Ok(None),
            (None, Some(_)) => bail!(
                "{FISH_CONFIG_SOURCE_ENV} is required when {STARSHIP_CONFIG_SOURCE_ENV} is set"
            ),
            (Some(_), None) => bail!(
                "{STARSHIP_CONFIG_SOURCE_ENV} is required when {FISH_CONFIG_SOURCE_ENV} is set"
            ),
        }
    }
}

pub(in crate::guest_init) fn materialize_configs(identity: &DevIdentity) -> Result<()> {
    materialize_configs_with_ownership(identity, true)
}

pub(in crate::guest_init) fn materialize_configs_with_ownership(
    identity: &DevIdentity,
    set_ownership: bool,
) -> Result<()> {
    let Some(sources) = ShellConfigSources::from_env()? else {
        return Ok(());
    };
    materialize_config_files(identity, &sources, set_ownership)
}

pub(in crate::guest_init) fn materialize_config_files(
    identity: &DevIdentity,
    sources: &ShellConfigSources,
    set_ownership: bool,
) -> Result<()> {
    let config_dir = identity.home.join(".config");
    let fish_config_dir = config_dir.join("fish");
    let fish_conf_dir = fish_config_dir.join("conf.d");
    let fish_data_dir = identity.home.join(".local/share/fish");
    let fish_completions_dir = fish_config_dir.join("completions");
    let fish_functions_dir = fish_config_dir.join("functions");

    for path in [
        fish_config_dir.as_path(),
        fish_conf_dir.as_path(),
        fish_completions_dir.as_path(),
        fish_functions_dir.as_path(),
        fish_data_dir.as_path(),
    ] {
        create_dir_for_identity(path, identity, set_ownership)?;
    }

    starship::materialize_config(
        identity,
        &sources.starship_config,
        &config_dir,
        set_ownership,
    )?;
    copy_config_file_if_missing(
        &sources.fish_config,
        &fish_conf_dir.join("agentbox-starship.fish"),
        identity,
        set_ownership,
    )
    .context("failed to materialize bundled fish starship config")?;

    Ok(())
}

fn create_dir_for_identity(path: &Path, identity: &DevIdentity, set_ownership: bool) -> Result<()> {
    fs::create_dir_all(path)?;
    if set_ownership {
        fs::chown(path, identity.uid, identity.gid)?;
    }
    Ok(())
}

fn copy_config_file_if_missing(
    source: &Path,
    target: &Path,
    identity: &DevIdentity,
    set_ownership: bool,
) -> Result<()> {
    if target.exists() {
        return Ok(());
    }
    if let Some(parent) = target.parent() {
        create_dir_for_identity(parent, identity, set_ownership)?;
    }
    std::fs::copy(source, target).with_context(|| {
        format!(
            "failed to copy {} to {}",
            source.display(),
            target.display()
        )
    })?;
    fs::chmod(target, 0o644)?;
    if set_ownership {
        fs::chown(target, identity.uid, identity.gid)?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "fish_tests.rs"]
mod tests;
