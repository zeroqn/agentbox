use anyhow::{Context, Result, bail};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use crate::runtime::launch_config::LaunchConfig;
use crate::runtime::runtime_etc::{self, RuntimeEtcFiles};

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
    runtime_etc: RuntimeEtcFiles,
}

impl PreparedRootPlan {
    fn new(config: &LaunchConfig, task_state_dir: &Path) -> Result<Self> {
        Self::new_with_runtime_etc(
            config,
            task_state_dir,
            runtime_etc::build(&config.hostname)?,
        )
    }

    fn new_with_runtime_etc(
        config: &LaunchConfig,
        task_state_dir: &Path,
        runtime_etc: RuntimeEtcFiles,
    ) -> Result<Self> {
        let root_export = task_state_dir.join(PREPARED_ROOT_DIR);
        let mut bind_grafts = Vec::with_capacity(
            config.mounts.len() + 1 + usize::from(config.guest_init_override.is_some()),
        );
        bind_grafts.push(PreparedRootBind {
            source: config.task_rootfs.clone(),
            target: root_export.clone(),
            kind: PreparedRootBindKind::Directory,
        });
        for mount in &config.mounts {
            bind_grafts.push(PreparedRootBind {
                source: mount.source.clone(),
                target: prepared_target(&root_export, &mount.target)?,
                kind: PreparedRootBindKind::Directory,
            });
        }
        if let Some(mount) = &config.guest_init_override {
            bind_grafts.push(PreparedRootBind {
                source: mount.source.clone(),
                target: prepared_target(&root_export, &mount.target)?,
                kind: PreparedRootBindKind::ReadOnlyFile,
            });
        }
        Ok(Self {
            root_export,
            bind_grafts,
            runtime_etc,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PreparedRootBind {
    source: PathBuf,
    target: PathBuf,
    kind: PreparedRootBindKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PreparedRootBindKind {
    Directory,
    ReadOnlyFile,
}

trait PreparedRootCommands {
    fn create_dir_all(&self, path: &Path) -> Result<()>;
    fn bind_mount(&self, source: &Path, target: &Path) -> Result<()>;
    fn remount_read_only(&self, target: &Path) -> Result<()>;
    fn materialize_runtime_etc(&self, root_export: &Path, files: &RuntimeEtcFiles) -> Result<()>;

    fn apply(&self, plan: &PreparedRootPlan) -> Result<()> {
        for bind in &plan.bind_grafts {
            match bind.kind {
                PreparedRootBindKind::Directory => {
                    validate_source_dir(&bind.source)?;
                    self.create_dir_all(&bind.target)?;
                    self.bind_mount(&bind.source, &bind.target)?;
                }
                PreparedRootBindKind::ReadOnlyFile => {
                    validate_source_file(&bind.source)?;
                    self.bind_mount(&bind.source, &bind.target)?;
                    self.remount_read_only(&bind.target)?;
                }
            }
        }
        self.materialize_runtime_etc(&plan.root_export, &plan.runtime_etc)?;
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

    fn remount_read_only(&self, target: &Path) -> Result<()> {
        let status = Command::new("mount")
            .args(["-o", "remount,bind,ro"])
            .arg(target)
            .status()
            .with_context(|| {
                format!(
                    "failed to run rootless prepared-root read-only remount '{}'",
                    target.display()
                )
            })?;
        if !status.success() {
            bail!(
                "rootless prepared-root read-only remount '{}' failed with {status}",
                target.display()
            );
        }
        Ok(())
    }

    fn materialize_runtime_etc(&self, root_export: &Path, files: &RuntimeEtcFiles) -> Result<()> {
        runtime_etc::materialize(root_export, files)
    }
}

fn validate_source_file(source: &Path) -> Result<()> {
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
    if !metadata.is_file() {
        bail!(
            "loftd prepared-root bind source '{}' must be a file",
            source.display()
        );
    }
    Ok(())
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

fn prepared_target(root_export: &Path, mount_target: &str) -> Result<PathBuf> {
    let target = Path::new(mount_target);
    if !target.is_absolute() {
        bail!(
            "loftd prepared-root bind target '{}' must be absolute",
            mount_target
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
                mount_target
            ),
            Component::Prefix(_) => bail!(
                "loftd prepared-root bind target '{}' has an unsupported prefix",
                mount_target
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
        BindMount, CARGO_TAG, CARGO_TARGET, CODEX_TAG, CODEX_TARGET, GuestInitOverrideMount,
        LaunchSpec, NetworkMode, PI_TAG, PI_TARGET, SCCACHE_TAG, SCCACHE_TARGET, WORKSPACE_TAG,
        WORKSPACE_TARGET,
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
        RuntimeEtc(String),
        ReadOnly(String),
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

        fn remount_read_only(&self, target: &Path) -> Result<()> {
            self.calls
                .borrow_mut()
                .push(Call::ReadOnly(target.display().to_string()));
            Ok(())
        }

        fn materialize_runtime_etc(
            &self,
            root_export: &Path,
            _files: &RuntimeEtcFiles,
        ) -> Result<()> {
            self.calls
                .borrow_mut()
                .push(Call::RuntimeEtc(root_export.display().to_string()));
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
            hostname: "loftd-workspace",
            mounts: &test_mounts(root),
            guest_init_override: None,
            guest_init_exec: "/nix/store/hash-loftd/bin/loftd-guest-init",
            guest_command: &[],
            image_process_config: &crate::runtime::image_source::OciProcessConfig::default(),
            mem_gib: Some(4),
            log_level: LogLevel::Off,
            network_mode: NetworkMode::Tsi,
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

    fn runtime_etc() -> RuntimeEtcFiles {
        RuntimeEtcFiles {
            hostname: "loftd-workspace\n".to_owned(),
            hosts:
                "127.0.0.1\tlocalhost loftd-workspace\n::1\tlocalhost ip6-localhost ip6-loopback\n"
                    .to_owned(),
            resolv_conf: "nameserver 192.0.2.53\n".to_owned(),
        }
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
        let plan =
            PreparedRootPlan::new_with_runtime_etc(&config, &state, runtime_etc()).expect("plan");
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
        assert_eq!(calls[12], Call::RuntimeEtc(root.display().to_string()));
    }

    #[test]
    fn prepared_root_plan_applies_guest_init_override_as_read_only_file_bind() {
        let dir = tempfile::tempdir().expect("tempdir");
        for path in [
            "task/rootfs/nix/store/hash-loftd/bin",
            "workspace-src",
            "home/.codex",
            "home/.pi",
            "state/cargo",
            "state/sccache",
        ] {
            fs::create_dir_all(dir.path().join(path)).expect("source dir");
        }
        let image_target = dir
            .path()
            .join("task/rootfs/nix/store/hash-loftd/bin/loftd-guest-init");
        fs::write(&image_target, "#!/bin/sh\n").expect("image guest init");
        let override_path = dir.path().join("override-loftd-guest-init");
        fs::write(&override_path, "#!/bin/sh\necho override\n").expect("override guest init");

        let state = dir.path().join("task");
        let mut config = config(dir.path());
        config.guest_init_override = Some(GuestInitOverrideMount {
            source: override_path.clone(),
            target: "/nix/store/hash-loftd/bin/loftd-guest-init".to_owned(),
            read_only: true,
        });
        let plan =
            PreparedRootPlan::new_with_runtime_etc(&config, &state, runtime_etc()).expect("plan");
        let commands = RecordingCommands::default();

        commands.apply(&plan).expect("apply should plan commands");

        let root = state.join(PREPARED_ROOT_DIR);
        let calls = commands.calls.borrow();
        assert_eq!(
            calls[12],
            Call::Bind(
                override_path.display().to_string(),
                root.join("nix/store/hash-loftd/bin/loftd-guest-init")
                    .display()
                    .to_string()
            )
        );
        assert_eq!(
            calls[13],
            Call::ReadOnly(
                root.join("nix/store/hash-loftd/bin/loftd-guest-init")
                    .display()
                    .to_string()
            )
        );
        assert_eq!(calls[14], Call::RuntimeEtc(root.display().to_string()));
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
        assert!(prepared_target(&root, &mount.target).is_err());

        mount.target = "/".to_owned();
        assert!(prepared_target(&root, &mount.target).is_err());

        mount.target = "/workspace/../escape".to_owned();
        assert!(prepared_target(&root, &mount.target).is_err());
    }
}
