use anyhow::{anyhow, Context, Result};
use std::env;
use std::path::{Path, PathBuf};

use crate::guest_init::root::home::DevIdentity;

pub(in crate::guest_init) const DEV_USER: &str = "dev";
pub(in crate::guest_init) const DEV_HOME: &str = "/home/dev";
pub(in crate::guest_init) const DEFAULT_SHELL: &str = "fish";
pub(in crate::guest_init) const RUN_DIR: &str = "/run/agentbox";
pub(in crate::guest_init) const PODMAN_STATUS_PATH: &str = "/run/agentbox/podman-prep.status";
pub(in crate::guest_init) const PODMAN_LOG_PATH: &str = "/run/agentbox/podman-prep.log";
pub(in crate::guest_init) const PODMAN_WAIT_TIMEOUT_SECS: u64 = 120;
pub(in crate::guest_init) const PASST_DNS_LINE: &str = "nameserver 169.254.1.1";
pub(in crate::guest_init) const RAW_NIX_DISK_ID: &str = "agentbox-nix";
pub(in crate::guest_init) const RAW_NIX_DISK_LABEL: &str = "AGENTBOX_NIX";
pub(in crate::guest_init) const RAW_CONTAINER_DISK_ID: &str = "agentbox-containers";
pub(in crate::guest_init) const RAW_CONTAINER_DISK_LABEL: &str = "AGENTBOX_CONTAINERS";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::guest_init) struct LibkrunEnv {
    pub(in crate::guest_init) nix_overlay: bool,
    pub(in crate::guest_init) containers_storage: bool,
    pub(in crate::guest_init) use_passt: bool,
    pub(in crate::guest_init) host_uid: Option<u32>,
    pub(in crate::guest_init) host_gid: Option<u32>,
    pub(in crate::guest_init) nix_disk_id: String,
    pub(in crate::guest_init) nix_disk_label: String,
    pub(in crate::guest_init) containers_disk_id: String,
    pub(in crate::guest_init) containers_disk_label: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::guest_init) struct ShellEnvironment {
    pub(in crate::guest_init) vars: Vec<(String, String)>,
    pub(in crate::guest_init) tmpdir: PathBuf,
    pub(in crate::guest_init) runtime_dir: Option<PathBuf>,
}

impl LibkrunEnv {
    pub(in crate::guest_init) fn from_process_env() -> Result<Self> {
        Ok(Self {
            nix_overlay: env_flag("AGENTBOX_LIBKRUN_NIX_OVERLAY"),
            containers_storage: env_flag("AGENTBOX_LIBKRUN_CONTAINERS_STORAGE"),
            use_passt: env_flag("AGENTBOX_LIBKRUN_USE_PASST"),
            host_uid: parse_optional_u32("AGENTBOX_HOST_UID")?,
            host_gid: parse_optional_u32("AGENTBOX_HOST_GID")?,
            nix_disk_id: env::var("AGENTBOX_LIBKRUN_NIX_DISK_ID")
                .unwrap_or_else(|_| RAW_NIX_DISK_ID.to_owned()),
            nix_disk_label: env::var("AGENTBOX_LIBKRUN_NIX_DISK_LABEL")
                .unwrap_or_else(|_| RAW_NIX_DISK_LABEL.to_owned()),
            containers_disk_id: env::var("AGENTBOX_LIBKRUN_CONTAINERS_DISK_ID")
                .unwrap_or_else(|_| RAW_CONTAINER_DISK_ID.to_owned()),
            containers_disk_label: env::var("AGENTBOX_LIBKRUN_CONTAINERS_DISK_LABEL")
                .unwrap_or_else(|_| RAW_CONTAINER_DISK_LABEL.to_owned()),
        })
    }

    pub(in crate::guest_init) fn require_host_identity(&self) -> Result<(u32, u32)> {
        let uid = self
            .host_uid
            .ok_or_else(|| anyhow!("AGENTBOX_HOST_UID is required for libkrun guest init"))?;
        let gid = self
            .host_gid
            .ok_or_else(|| anyhow!("AGENTBOX_HOST_GID is required for libkrun guest init"))?;
        Ok((uid, gid))
    }
}

pub(in crate::guest_init) fn derive_shell_environment(
    identity: &DevIdentity,
    containers_storage: bool,
) -> ShellEnvironment {
    let home = identity.home.display().to_string();
    let tmpdir = identity.home.join(".cache/tmp");
    let runtime_dir =
        containers_storage.then(|| PathBuf::from(format!("/run/user/{}", identity.uid)));
    let mut vars = vec![
        ("USER".to_owned(), DEV_USER.to_owned()),
        ("HOME".to_owned(), home.clone()),
        ("SHELL".to_owned(), identity.shell.display().to_string()),
        ("XDG_CONFIG_HOME".to_owned(), format!("{home}/.config")),
        ("XDG_DATA_HOME".to_owned(), format!("{home}/.local/share")),
        ("XDG_STATE_HOME".to_owned(), format!("{home}/.local/state")),
        ("XDG_CACHE_HOME".to_owned(), format!("{home}/.cache")),
        ("TMPDIR".to_owned(), tmpdir.display().to_string()),
    ];
    if let Some(runtime_dir) = &runtime_dir {
        vars.push((
            "XDG_RUNTIME_DIR".to_owned(),
            runtime_dir.display().to_string(),
        ));
    }
    if containers_storage {
        let path = env::var("PATH").unwrap_or_default();
        vars.push(("PATH".to_owned(), format!("/run/agentbox/idmap-bin:{path}")));
    }
    ShellEnvironment {
        vars,
        tmpdir,
        runtime_dir,
    }
}

pub(in crate::guest_init) fn export_shell_environment(env_contract: &ShellEnvironment) {
    for (key, value) in &env_contract.vars {
        env::set_var(key, value);
    }
}

pub(in crate::guest_init) fn normalize_resolv_conf(input: Option<&str>) -> String {
    let mut out = String::from(PASST_DNS_LINE);
    out.push('\n');
    if let Some(input) = input {
        for line in input.lines() {
            if line != PASST_DNS_LINE {
                out.push_str(line);
                out.push('\n');
            }
        }
    }
    out
}

pub(in crate::guest_init) fn ensure_passt_resolv_conf(path: &Path) -> Result<()> {
    let current = match std::fs::read_to_string(path) {
        Ok(text) => Some(text),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
        Err(err) => return Err(err).with_context(|| format!("failed to read {}", path.display())),
    };
    crate::guest_init::fs::write_file(path, &normalize_resolv_conf(current.as_deref()), 0o644)
}

fn env_flag(name: &str) -> bool {
    env::var(name).as_deref() == Ok("1")
}

fn parse_optional_u32(name: &str) -> Result<Option<u32>> {
    match env::var(name) {
        Ok(value) if !value.is_empty() => {
            Ok(Some(value.parse().with_context(|| {
                format!("invalid numeric value in {name}")
            })?))
        }
        _ => Ok(None),
    }
}

#[cfg(test)]
#[path = "libkrun_tests.rs"]
mod tests;
