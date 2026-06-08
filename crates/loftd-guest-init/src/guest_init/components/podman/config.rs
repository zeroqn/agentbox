use anyhow::{Result, anyhow};
use std::path::{Path, PathBuf};

use crate::guest_init::command;
use crate::guest_init::components::env::ContainerStoreBackend;
use crate::guest_init::components::home::identity::DevIdentity;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::guest_init) struct PodmanToolPaths {
    pub(in crate::guest_init) conmon: PathBuf,
    pub(in crate::guest_init) crun: PathBuf,
    pub(in crate::guest_init) netavark_dir: PathBuf,
    pub(in crate::guest_init) aardvark_dns_dir: PathBuf,
    pub(in crate::guest_init) pasta_dir: PathBuf,
}

impl PodmanToolPaths {
    pub(in crate::guest_init) fn discover() -> Result<Self> {
        Ok(Self {
            conmon: command::require_on_path("conmon")?,
            crun: command::require_on_path("crun")?,
            netavark_dir: parent_dir(&command::require_on_path("netavark")?)?,
            aardvark_dns_dir: parent_dir(&command::require_on_path("aardvark-dns")?)?,
            pasta_dir: parent_dir(&command::require_on_path("pasta")?)?,
        })
    }

    #[cfg(test)]
    pub(in crate::guest_init) fn fixture() -> Self {
        Self {
            conmon: PathBuf::from("/nix/store/conmon/bin/conmon"),
            crun: PathBuf::from("/nix/store/crun/bin/crun"),
            netavark_dir: PathBuf::from("/nix/store/netavark/bin"),
            aardvark_dns_dir: PathBuf::from("/nix/store/aardvark-dns/bin"),
            pasta_dir: PathBuf::from("/nix/store/passt/bin"),
        }
    }
}

pub(in crate::guest_init) fn storage_conf(
    identity: &DevIdentity,
    container_store_backend: ContainerStoreBackend,
) -> String {
    let graphroot = "/home/dev/.local/share/containers/storage";
    let runroot = format!("/run/user/{}/containers", identity.uid);
    let driver = match container_store_backend {
        ContainerStoreBackend::Bind => "overlay",
        ContainerStoreBackend::RawDisk => "btrfs",
    };
    format!(
        r#"[storage]
driver = "{driver}"
graphroot = "{graphroot}"
runroot = "{runroot}"
"#
    )
}

pub(in crate::guest_init) fn containers_conf(paths: &PodmanToolPaths) -> String {
    format!(
        r#"[containers]
cgroups = "disabled"

[engine]
cgroup_manager = "cgroupfs"
compose_warning_logs = false
events_logger = "file"
runtime = "crun"
conmon_path = ["{}"]
helper_binaries_dir = ["{}", "{}", "{}", "/run/loftd/idmap-bin"]

[engine.runtimes]
crun = ["{}"]

[network]
network_backend = "netavark"
"#,
        paths.conmon.display(),
        paths.netavark_dir.display(),
        paths.aardvark_dns_dir.display(),
        paths.pasta_dir.display(),
        paths.crun.display()
    )
}

pub(in crate::guest_init) fn registries_conf() -> &'static str {
    r#"[registries.block]
registries = []

[registries.insecure]
registries = []

[registries.search]
registries = ["docker.io"]
"#
}

pub(in crate::guest_init) fn policy_json() -> &'static str {
    r#"{
  "default": [
    {
      "type": "insecureAcceptAnything"
    }
  ],
  "transports": {
    "docker-daemon": {
      "": [
        {
          "type": "insecureAcceptAnything"
        }
      ]
    }
  }
}
"#
}

fn parent_dir(path: &Path) -> Result<PathBuf> {
    path.parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| anyhow!("tool path has no parent directory: {}", path.display()))
}

#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;
