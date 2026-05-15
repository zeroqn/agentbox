use std::path::PathBuf;

use crate::guest_init::components::env::DEV_HOME;
use crate::guest_init::components::home::identity::DevIdentity;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::guest_init) struct DockerPaths {
    pub(in crate::guest_init) config_dir: PathBuf,
    pub(in crate::guest_init) daemon_config: PathBuf,
    pub(in crate::guest_init) data_root: PathBuf,
    pub(in crate::guest_init) exec_root: PathBuf,
    pub(in crate::guest_init) state_root: PathBuf,
    pub(in crate::guest_init) runtime_dir: PathBuf,
    pub(in crate::guest_init) socket_path: PathBuf,
    pub(in crate::guest_init) pid_path: PathBuf,
    pub(in crate::guest_init) daemon_status_path: PathBuf,
    pub(in crate::guest_init) daemon_log_path: PathBuf,
    pub(in crate::guest_init) daemon_start_lock_path: PathBuf,
}

impl DockerPaths {
    pub(in crate::guest_init) fn for_identity(identity: &DevIdentity) -> Self {
        let container_root = PathBuf::from(DEV_HOME).join(".local/share/containers/docker");
        let user_runtime_dir = PathBuf::from(format!("/run/user/{}", identity.uid));
        let runtime_dir = user_runtime_dir.join("docker");
        Self {
            config_dir: PathBuf::from(DEV_HOME).join(".config/docker"),
            daemon_config: PathBuf::from(DEV_HOME).join(".config/docker/daemon.json"),
            data_root: container_root.join("data"),
            exec_root: container_root.join("exec"),
            state_root: container_root.join("state"),
            socket_path: user_runtime_dir.join("docker.sock"),
            pid_path: runtime_dir.join("dockerd-rootless.pid"),
            daemon_status_path: runtime_dir.join("daemon.status"),
            daemon_log_path: runtime_dir.join("daemon.log"),
            daemon_start_lock_path: runtime_dir.join("daemon-start.lock"),
            runtime_dir,
        }
    }

    pub(in crate::guest_init) fn host_uri(&self) -> String {
        format!("unix://{}", self.socket_path.display())
    }
}

pub(in crate::guest_init) fn daemon_json(paths: &DockerPaths) -> String {
    format!(
        r#"{{
  "storage-driver": "btrfs",
  "data-root": "{}",
  "exec-root": "{}",
  "pidfile": "{}",
  "features": {{
    "containerd-snapshotter": false
  }}
}}
"#,
        paths.data_root.display(),
        paths.exec_root.display(),
        paths.pid_path.display()
    )
}

#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;
