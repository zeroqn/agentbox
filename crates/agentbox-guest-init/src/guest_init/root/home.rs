use anyhow::{bail, Context, Result};
use std::env;
use std::path::{Path, PathBuf};

use crate::guest_init::runtime::libkrun::{DEV_HOME, DEV_USER};
use crate::guest_init::{fs, process};

const FISH_CONFIG_SOURCE_ENV: &str = "AGENTBOX_FISH_CONFIG_SOURCE";
const STARSHIP_CONFIG_SOURCE_ENV: &str = "AGENTBOX_STARSHIP_CONFIG_SOURCE";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::guest_init) struct DevIdentity {
    pub(in crate::guest_init) uid: u32,
    pub(in crate::guest_init) gid: u32,
    pub(in crate::guest_init) home: PathBuf,
    pub(in crate::guest_init) shell: PathBuf,
}

impl DevIdentity {
    pub(in crate::guest_init) fn new(uid: u32, gid: u32, shell: PathBuf) -> Self {
        Self {
            uid,
            gid,
            home: PathBuf::from(DEV_HOME),
            shell,
        }
    }
}

pub(in crate::guest_init) fn materialize(identity: &DevIdentity) -> Result<()> {
    if !process::is_root() {
        return Ok(());
    }
    let passwd = build_passwd(identity)?;
    let group = build_group(identity)?;
    fs::write_file(Path::new("/etc/passwd"), &passwd, 0o644)
        .context("failed to materialize dynamic dev entry in /etc/passwd")?;
    fs::write_file(Path::new("/etc/group"), &group, 0o644)
        .context("failed to materialize dynamic dev entry in /etc/group")?;
    ensure_home_dirs(identity)?;
    materialize_shell_configs(identity)
}

pub(in crate::guest_init) fn ensure_home_dirs(identity: &DevIdentity) -> Result<()> {
    for path in [
        identity.home.as_path(),
        Path::new("/home/dev/.local"),
        Path::new("/home/dev/.local/share"),
        Path::new("/home/dev/.local/state"),
        Path::new("/home/dev/.cache"),
        Path::new("/home/dev/.cache/tmp"),
        Path::new("/home/dev/.config"),
    ] {
        fs::create_dir_all(path)?;
        fs::chown(path, identity.uid, identity.gid)?;
    }
    fs::chmod(Path::new("/home/dev/.local/state"), 0o700)?;
    fs::chmod(Path::new("/home/dev/.cache/tmp"), 0o700)?;
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ShellConfigSources {
    fish_config: PathBuf,
    starship_config: PathBuf,
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

fn materialize_shell_configs(identity: &DevIdentity) -> Result<()> {
    let Some(sources) = ShellConfigSources::from_env()? else {
        return Ok(());
    };
    materialize_shell_config_files(identity, &sources, true)
}

fn materialize_shell_config_files(
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
    let starship_cache_dir = identity.home.join(".cache/starship");

    for path in [
        fish_config_dir.as_path(),
        fish_conf_dir.as_path(),
        fish_completions_dir.as_path(),
        fish_functions_dir.as_path(),
        fish_data_dir.as_path(),
        starship_cache_dir.as_path(),
    ] {
        create_dir_for_identity(path, identity, set_ownership)?;
    }

    copy_config_file_if_missing(
        &sources.starship_config,
        &config_dir.join("starship.toml"),
        identity,
        set_ownership,
    )
    .context("failed to materialize bundled starship config")?;
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

fn build_passwd(identity: &DevIdentity) -> Result<String> {
    let existing = read_without_dev(Path::new("/etc/passwd"))?;
    Ok(format!(
        "{existing}{DEV_USER}:x:{}:{}:dev user:{}:{}\n",
        identity.uid,
        identity.gid,
        identity.home.display(),
        identity.shell.display()
    ))
}

fn build_group(identity: &DevIdentity) -> Result<String> {
    let existing = read_without_dev(Path::new("/etc/group"))?;
    Ok(format!("{existing}{DEV_USER}:x:{}:\n", identity.gid))
}

fn read_without_dev(path: &Path) -> Result<String> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(err) => return Err(err).with_context(|| format!("failed to read {}", path.display())),
    };
    let mut out = String::new();
    for line in text.lines() {
        if !line.starts_with("dev:") {
            out.push_str(line);
            out.push('\n');
        }
    }
    Ok(out)
}

pub(in crate::guest_init) fn validate_host_identity(uid: u32, gid: u32) -> Result<()> {
    if uid == 0 || gid == 0 {
        bail!("libkrun host UID/GID must identify the non-root dev user, got {uid}:{gid}");
    }
    Ok(())
}

#[cfg(test)]
#[path = "home_tests.rs"]
mod tests;
