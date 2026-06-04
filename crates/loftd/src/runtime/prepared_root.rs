use anyhow::{Context, Result, bail};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use crate::runtime::launch_config::{BindMount, LaunchConfig};

const PREPARED_ROOT_DIR: &str = "prepared-root";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PreparedRoot {
    root: PathBuf,
}

impl PreparedRoot {
    pub(crate) fn root(&self) -> &Path {
        &self.root
    }
}

pub(crate) fn prepare(config: &LaunchConfig, task_state_dir: &Path) -> Result<PreparedRoot> {
    let plan = PreparedRootPlan::new(config, task_state_dir)?;
    HostPreparedRootCommands.apply(&plan)?;
    Ok(PreparedRoot {
        root: plan.root_export,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PreparedRootPlan {
    root_export: PathBuf,
    bind_grafts: Vec<PreparedRootBind>,
}

impl PreparedRootPlan {
    fn new(config: &LaunchConfig, task_state_dir: &Path) -> Result<Self> {
        let root_export = task_state_dir.join(PREPARED_ROOT_DIR);
        let mut bind_grafts = Vec::with_capacity(config.mounts.len() + 1);
        bind_grafts.push(PreparedRootBind {
            source: config.task_rootfs.clone(),
            target: root_export.clone(),
        });
        for mount in &config.mounts {
            bind_grafts.push(PreparedRootBind {
                source: mount.source.clone(),
                target: prepared_target(&root_export, mount)?,
            });
        }
        Ok(Self {
            root_export,
            bind_grafts,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PreparedRootBind {
    source: PathBuf,
    target: PathBuf,
}

trait PreparedRootCommands {
    fn create_dir_all(&self, path: &Path) -> Result<()>;
    fn bind_mount(&self, source: &Path, target: &Path) -> Result<()>;

    fn apply(&self, plan: &PreparedRootPlan) -> Result<()> {
        for bind in &plan.bind_grafts {
            validate_source_dir(&bind.source)?;
            self.create_dir_all(&bind.target)?;
            self.bind_mount(&bind.source, &bind.target)?;
        }
        Ok(())
    }
}

struct HostPreparedRootCommands;

impl PreparedRootCommands for HostPreparedRootCommands {
    fn create_dir_all(&self, path: &Path) -> Result<()> {
        fs::create_dir_all(path)
            .with_context(|| format!("failed to create prepared root path '{}'", path.display()))
    }

    fn bind_mount(&self, source: &Path, target: &Path) -> Result<()> {
        let status = Command::new("mount")
            .args(["--bind"])
            .arg(source)
            .arg(target)
            .status()
            .with_context(|| {
                format!(
                    "failed to run rootless prepared-root bind mount '{}'=>'{}'",
                    source.display(),
                    target.display()
                )
            })?;
        if !status.success() {
            bail!(
                "rootless prepared-root bind mount '{}'=>'{}' failed with {status}",
                source.display(),
                target.display()
            );
        }
        Ok(())
    }
}

fn validate_source_dir(source: &Path) -> Result<()> {
    if !source.is_absolute() {
        bail!(
            "loftd prepared-root bind source '{}' must be absolute",
            source.display()
        );
    }
    let metadata = fs::metadata(source).with_context(|| {
        format!(
            "failed to inspect prepared-root source '{}'",
            source.display()
        )
    })?;
    if !metadata.is_dir() {
        bail!(
            "loftd prepared-root bind source '{}' must be a directory",
            source.display()
        );
    }
    Ok(())
}

fn prepared_target(root_export: &Path, mount: &BindMount) -> Result<PathBuf> {
    let target = Path::new(&mount.target);
    if !target.is_absolute() {
        bail!(
            "loftd prepared-root bind target '{}' must be absolute",
            mount.target
        );
    }
    let mut relative = PathBuf::new();
    for component in target.components() {
        match component {
            Component::RootDir => {}
            Component::Normal(value) => relative.push(value),
            Component::CurDir => {}
            Component::ParentDir => bail!(
                "loftd prepared-root bind target '{}' must not contain '..'",
                mount.target
            ),
            Component::Prefix(_) => bail!(
                "loftd prepared-root bind target '{}' has an unsupported prefix",
                mount.target
            ),
        }
    }
    if relative.as_os_str().is_empty() {
        bail!("loftd prepared-root bind target must not be /");
    }
    Ok(root_export.join(relative))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logging::LogLevel;
    use crate::runtime::launch_config::{
        CARGO_TAG, CARGO_TARGET, CODEX_TAG, CODEX_TARGET, LaunchSpec, PI_TAG, PI_TARGET,
        SCCACHE_TAG, SCCACHE_TARGET, WORKSPACE_TAG, WORKSPACE_TARGET,
    };
    use std::cell::RefCell;
    use std::path::Path;

    #[derive(Default)]
    struct RecordingCommands {
        calls: RefCell<Vec<Call>>,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum Call {
        CreateDir(String),
        Bind(String, String),
    }

    impl PreparedRootCommands for RecordingCommands {
        fn create_dir_all(&self, path: &Path) -> Result<()> {
            self.calls
                .borrow_mut()
                .push(Call::CreateDir(path.display().to_string()));
            Ok(())
        }

        fn bind_mount(&self, source: &Path, target: &Path) -> Result<()> {
            self.calls.borrow_mut().push(Call::Bind(
                source.display().to_string(),
                target.display().to_string(),
            ));
            Ok(())
        }
    }

    fn test_mounts(root: &Path) -> Vec<BindMount> {
        vec![
            BindMount {
                source: root.join("workspace-src"),
                tag: WORKSPACE_TAG.to_owned(),
                target: WORKSPACE_TARGET.to_owned(),
            },
            BindMount {
                source: root.join("home/.codex"),
                tag: CODEX_TAG.to_owned(),
                target: CODEX_TARGET.to_owned(),
            },
            BindMount {
                source: root.join("home/.pi"),
                tag: PI_TAG.to_owned(),
                target: PI_TARGET.to_owned(),
            },
            BindMount {
                source: root.join("state/cargo"),
                tag: CARGO_TAG.to_owned(),
                target: CARGO_TARGET.to_owned(),
            },
            BindMount {
                source: root.join("state/sccache"),
                tag: SCCACHE_TAG.to_owned(),
                target: SCCACHE_TARGET.to_owned(),
            },
        ]
    }

    fn config(root: &Path) -> LaunchConfig {
        LaunchConfig::build_for_task(LaunchSpec {
            task_rootfs: &root.join("task/rootfs"),
            mounts: &test_mounts(root),
            guest_init_exec: "/nix/store/hash-loftd/bin/loftd-guest-init",
            guest_command: &[],
            image_process_config: &crate::runtime::image_source::OciProcessConfig::default(),
            mem_gib: Some(4),
            log_level: LogLevel::Off,
            profile: false,
            root: false,
            host_uid: 1000,
            host_gid: 1001,
            vcpus: 2,
            disks: Vec::new(),
            extra_env: Vec::new(),
        })
        .expect("config should build")
    }

    #[test]
    fn prepared_root_plan_binds_task_root_before_developer_grafts() {
        let dir = tempfile::tempdir().expect("tempdir");
        for path in [
            "task/rootfs",
            "workspace-src",
            "home/.codex",
            "home/.pi",
            "state/cargo",
            "state/sccache",
        ] {
            fs::create_dir_all(dir.path().join(path)).expect("source dir");
        }
        let state = dir.path().join("task");
        let config = config(dir.path());
        let plan = PreparedRootPlan::new(&config, &state).expect("plan");
        let commands = RecordingCommands::default();

        commands.apply(&plan).expect("apply should plan commands");

        let root = state.join(PREPARED_ROOT_DIR);
        let calls = commands.calls.borrow();
        assert_eq!(calls[0], Call::CreateDir(root.display().to_string()));
        assert_eq!(
            calls[1],
            Call::Bind(
                dir.path().join("task/rootfs").display().to_string(),
                root.display().to_string()
            )
        );
        assert_eq!(
            calls[3],
            Call::Bind(
                dir.path().join("workspace-src").display().to_string(),
                root.join("workspace").display().to_string()
            )
        );
        assert_eq!(
            calls[5],
            Call::Bind(
                dir.path().join("home/.codex").display().to_string(),
                root.join("home/dev/.codex").display().to_string()
            )
        );
        assert_eq!(
            calls[11],
            Call::Bind(
                dir.path().join("state/sccache").display().to_string(),
                root.join("home/dev/.cache/sccache").display().to_string()
            )
        );
    }

    #[test]
    fn prepared_root_rejects_relative_or_root_targets() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().join("prepared-root");
        let mut mount = BindMount {
            source: dir.path().join("src"),
            tag: "bad".to_owned(),
            target: "workspace".to_owned(),
        };
        assert!(prepared_target(&root, &mount).is_err());

        mount.target = "/".to_owned();
        assert!(prepared_target(&root, &mount).is_err());

        mount.target = "/workspace/../escape".to_owned();
        assert!(prepared_target(&root, &mount).is_err());
    }
}
