use anyhow::{Context, Result};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use crate::cli::{ContainerStoreBackend, RuntimeOptions, VolumeSpec};
use crate::config;
use crate::logging::LogLevel;
use crate::naming::derive_workspace_slug;
use crate::runtime::host_tools;
use crate::runtime::launch::components::mounts;
use crate::runtime::launch::config::{
    BindMount, BindMountSourceKind, NIX_TARGET, NetworkMode, canonical_mount_target,
};
use crate::runtime::seccomp::{self, AuditMode, SeccompMode};
use crate::state::{self, StateLayout};
use crate::task_rootfs::TaskRootfsBackend;
use crate::{DEFAULT_FALLBACK_IMAGE, DEFAULT_IMAGE};

/// Resolved host-side launch intent and session inputs.
///
/// `LaunchPlan` is built from CLI/config/environment before any task rootfs or
/// helper/libkrun execution contract is materialized.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LaunchPlan {
    pub(crate) workspace_dir: PathBuf,
    pub(crate) workspace_slug: String,
    pub(crate) hostname: String,
    pub(crate) state_layout: StateLayout,
    pub(crate) image_cache_dir: PathBuf,
    pub(crate) sccache_dir: PathBuf,
    pub bind_mounts: Vec<BindMount>,
    pub(crate) image_selection: ImageSelection,
    pub(crate) task_rootfs_backend: TaskRootfsBackend,
    pub(crate) container_store_backend: ContainerStoreBackend,
    pub(crate) guest_init: Option<PathBuf>,
    pub(crate) mem_gib: Option<u32>,
    pub(crate) network_mode: NetworkMode,
    pub(crate) publish: Vec<String>,
    pub(crate) guest_command: Vec<String>,
    pub(crate) debug: bool,
    pub(crate) log_level: LogLevel,
    pub(crate) profile: bool,
    pub(crate) root: bool,
    pub(crate) daemon: bool,
    pub(crate) seccomp: SeccompMode,
    pub(crate) hardened: bool,
    pub(crate) preserve_debug: bool,
    pub(crate) config_diagnostics: ConfigDiagnostics,
}

impl LaunchPlan {
    pub(crate) fn from_env(options: RuntimeOptions, workspace_dir: PathBuf) -> Result<Self> {
        let xdg_state_home = env::var_os("XDG_STATE_HOME").map(PathBuf::from);
        let xdg_config_home = env::var_os("XDG_CONFIG_HOME").map(PathBuf::from);
        let home_dir = env::var_os("HOME").map(PathBuf::from);

        Self::from_env_values(
            options,
            workspace_dir,
            xdg_state_home.as_deref(),
            xdg_config_home.as_deref(),
            home_dir.as_deref(),
        )
    }

    fn from_env_values(
        options: RuntimeOptions,
        workspace_dir: PathBuf,
        xdg_state_home: Option<&Path>,
        xdg_config_home: Option<&Path>,
        home_dir: Option<&Path>,
    ) -> Result<Self> {
        let config = config::state::read_config(xdg_config_home, home_dir)?;
        let state_layout = state::resolve_state_layout_from_parts(
            &workspace_dir,
            xdg_state_home,
            home_dir,
            config.state_location_override(),
        )?;
        let workspace_slug = derive_workspace_slug(&workspace_dir);
        let hostname = derive_runtime_hostname(&workspace_slug);
        let image_cache_dir = state_layout.image_cache_dir();
        let sccache_dir = state_layout.sccache_dir();
        let home_dir = home_dir.ok_or_else(|| {
            anyhow::anyhow!(
                "HOME is not set; loftd cannot prepare .codex, .omp, and .pi bind mounts"
            )
        })?;
        let container_store_backend = options
            .container_store_backend
            .unwrap_or(ContainerStoreBackend::DEFAULT);
        let mut bind_mounts = mounts::prepare_dev_mounts(&workspace_dir, home_dir, &state_layout)?;
        bind_mounts.extend(prepare_user_volume_mounts(
            &options.volumes,
            &workspace_dir,
            bind_mounts.len(),
        )?);
        crate::runtime::launch::config::validate_mounts(&bind_mounts)?;
        let task_rootfs_backend = options
            .rootfs_backend
            .or_else(|| config.task_rootfs_backend())
            .unwrap_or(TaskRootfsBackend::DEFAULT);
        let seccomp = resolve_normal_launch_seccomp(options.seccomp)?;

        Ok(Self {
            workspace_dir,
            workspace_slug,
            hostname,
            state_layout,
            image_cache_dir,
            sccache_dir,
            bind_mounts,
            image_selection: ImageSelection::from_runtime_options(
                options.image,
                options.pull_latest,
            ),
            task_rootfs_backend,
            container_store_backend,
            guest_init: options.guest_init,
            mem_gib: options.mem_gib,
            network_mode: options.network_mode,
            publish: options.publish,
            guest_command: options.guest_command,
            debug: options.log_settings.level.enables_debug(),
            log_level: options.log_settings.level,
            profile: options.profile,
            root: options.root,
            daemon: options.daemon,
            seccomp,
            hardened: options.hardened,
            preserve_debug: options.preserve_debug,
            config_diagnostics: ConfigDiagnostics {
                config_path: config.path().to_path_buf(),
                config_loaded: config.loaded(),
            },
        })
    }
}

fn resolve_normal_launch_seccomp(seccomp: Option<SeccompMode>) -> Result<SeccompMode> {
    resolve_normal_launch_seccomp_with(seccomp, host_tools::default_seccomp_policy_path)
}

fn resolve_normal_launch_seccomp_with(
    seccomp: Option<SeccompMode>,
    default_policy_path: impl FnOnce() -> Option<PathBuf>,
) -> Result<SeccompMode> {
    match seccomp {
        Some(SeccompMode::Audit(AuditMode::DefaultGap { trace_path })) => {
            let baseline_policy_path = seccomp::resolve_default_seccomp_policy_path(
                default_policy_path,
                "default seccomp gap audit",
                "pass --seccomp=audit:POLICY_JSON:TRACE_JSONL explicitly",
            )?;
            Ok(SeccompMode::Audit(AuditMode::Gap {
                baseline_policy_path,
                trace_path,
            }))
        }
        Some(seccomp) => Ok(seccomp),
        None => {
            let policy_path = seccomp::resolve_default_seccomp_policy_path(
                default_policy_path,
                "normal launch seccomp enforcement",
                "pass --seccomp=off to disable host-side seccomp for this run",
            )?;
            Ok(SeccompMode::Enforce { policy_path })
        }
    }
}

fn prepare_user_volume_mounts(
    volumes: &[VolumeSpec],
    workspace_dir: &Path,
    tag_start: usize,
) -> Result<Vec<BindMount>> {
    volumes
        .iter()
        .enumerate()
        .map(|(index, volume)| prepare_user_volume_mount(volume, workspace_dir, tag_start + index))
        .collect()
}

fn prepare_user_volume_mount(
    volume: &VolumeSpec,
    workspace_dir: &Path,
    tag_index: usize,
) -> Result<BindMount> {
    let target = validate_user_volume_target(&volume.target)?;
    let source = absolute_source_path(&volume.source, workspace_dir);
    let source = fs::canonicalize(&source)
        .with_context(|| format!("failed to inspect volume source '{}'", source.display()))?;
    let metadata = fs::metadata(&source)
        .with_context(|| format!("failed to inspect volume source '{}'", source.display()))?;
    let source_kind = if metadata.is_dir() {
        BindMountSourceKind::Directory
    } else if metadata.is_file() {
        BindMountSourceKind::File
    } else {
        anyhow::bail!(
            "loftd volume source '{}' must be a file or directory",
            source.display()
        );
    };
    let tag = format!("loftd-user-volume-{tag_index}");
    match source_kind {
        BindMountSourceKind::Directory => Ok(BindMount {
            source,
            tag,
            target,
            source_kind,
            read_only: volume.read_only,
        }),
        BindMountSourceKind::File => Ok(BindMount::file(source, tag, target, volume.read_only)),
    }
}

fn absolute_source_path(source: &Path, workspace_dir: &Path) -> PathBuf {
    if source.is_absolute() {
        source.to_path_buf()
    } else {
        workspace_dir.join(source)
    }
}

fn validate_user_volume_target(target: &str) -> Result<String> {
    let target = canonical_mount_target(target)?;
    if target == NIX_TARGET {
        anyhow::bail!("loftd volume target {NIX_TARGET} is reserved");
    }
    if target.contains(".config/codex") {
        anyhow::bail!("loftd volume target must not include .config/codex");
    }
    Ok(target)
}

fn derive_runtime_hostname(workspace_slug: &str) -> String {
    format!("loftd-{workspace_slug}")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ConfigDiagnostics {
    pub(crate) config_path: PathBuf,
    pub(crate) config_loaded: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ImageSelection {
    PreferLocalhostThenCanonical,
    CanonicalWithRefresh,
    Explicit { reference: String },
}

impl ImageSelection {
    fn from_runtime_options(explicit_image: Option<String>, pull_latest: bool) -> Self {
        match explicit_image {
            Some(reference) => Self::Explicit { reference },
            None if pull_latest => Self::CanonicalWithRefresh,
            None => Self::PreferLocalhostThenCanonical,
        }
    }

    pub(crate) fn selected_reference(&self) -> &str {
        match self {
            Self::PreferLocalhostThenCanonical => DEFAULT_IMAGE,
            Self::CanonicalWithRefresh => DEFAULT_FALLBACK_IMAGE,
            Self::Explicit { reference } => reference,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};

    use crate::cli::{ContainerStoreBackend, RuntimeOptions, VolumeSpec};
    use crate::logging::{LogLevel, LogSettings};
    use crate::runtime::launch::config::{BindMountSourceKind, NetworkMode};
    use crate::runtime::launch::plan::{
        ImageSelection, LaunchPlan, resolve_normal_launch_seccomp_with,
    };
    use crate::runtime::seccomp::{AuditMode, SeccompMode};
    use crate::task_rootfs::TaskRootfsBackend;
    use crate::{DEFAULT_FALLBACK_IMAGE, DEFAULT_IMAGE};

    fn runtime_options() -> RuntimeOptions {
        RuntimeOptions {
            image: None,
            pull_latest: false,
            debug: false,
            log_settings: LogSettings::resolve(None, false, None),
            profile: false,
            root: false,
            daemon: false,
            seccomp: Some(SeccompMode::Off),
            hardened: false,
            rootfs_backend: None,
            container_store_backend: None,
            guest_init: None,
            preserve_debug: false,
            mem_gib: None,
            network_mode: NetworkMode::Tsi,
            publish: Vec::new(),
            volumes: Vec::new(),
            guest_command: Vec::new(),
        }
    }

    #[test]
    fn default_plan_prefers_local_image_and_btrfs_backend() {
        let dir = tempfile::tempdir().expect("tempdir should exist");
        let workspace = PathBuf::from("/tmp/example-project");
        let plan = LaunchPlan::from_env_values(
            runtime_options(),
            workspace.clone(),
            Some(dir.path().join("state").as_path()),
            Some(dir.path().join("config").as_path()),
            Some(dir.path().join("home").as_path()),
        )
        .expect("plan should build");

        assert_eq!(plan.workspace_dir, workspace);
        assert_eq!(plan.workspace_slug, "example-project");
        assert_eq!(plan.hostname, "loftd-example-project");
        assert_eq!(
            plan.image_selection,
            ImageSelection::PreferLocalhostThenCanonical
        );
        assert_eq!(plan.image_selection.selected_reference(), DEFAULT_IMAGE);
        assert_eq!(plan.task_rootfs_backend, TaskRootfsBackend::BtrfsSnapshot);
        assert_eq!(plan.container_store_backend, ContainerStoreBackend::RawDisk);
        assert_eq!(plan.log_level, LogLevel::Off);
        assert!(!plan.debug);
        assert!(!plan.daemon);
        assert!(!plan.config_diagnostics.config_loaded);
    }

    #[test]
    fn omitted_normal_launch_seccomp_resolves_to_valid_packaged_default_policy() {
        let dir = tempfile::tempdir().expect("tempdir should exist");
        let policy = dir.path().join("default.json");
        fs::write(
            &policy,
            include_bytes!("../../../assets/seccomp/default.json"),
        )
        .expect("default policy fixture should be written");
        let seccomp = resolve_normal_launch_seccomp_with(None, || Some(policy.clone()))
            .expect("default seccomp should resolve");

        assert_eq!(
            seccomp,
            SeccompMode::Enforce {
                policy_path: policy
            }
        );
    }

    #[test]
    fn default_gap_audit_resolves_to_valid_packaged_default_policy() {
        let dir = tempfile::tempdir().expect("tempdir should exist");
        let policy = dir.path().join("default.json");
        fs::write(
            &policy,
            include_bytes!("../../../assets/seccomp/default.json"),
        )
        .expect("default policy fixture should be written");
        let trace_path = dir.path().join("missing.jsonl");

        let seccomp = resolve_normal_launch_seccomp_with(
            Some(SeccompMode::Audit(AuditMode::DefaultGap {
                trace_path: trace_path.clone(),
            })),
            || Some(policy.clone()),
        )
        .expect("default gap audit should resolve");

        assert_eq!(
            seccomp,
            SeccompMode::Audit(AuditMode::Gap {
                baseline_policy_path: policy,
                trace_path,
            })
        );
    }

    #[test]
    fn default_gap_audit_fails_closed_without_default_policy() {
        let err = resolve_normal_launch_seccomp_with(
            Some(SeccompMode::Audit(AuditMode::DefaultGap {
                trace_path: PathBuf::from("missing.jsonl"),
            })),
            || None,
        )
        .expect_err("missing default policy should fail closed");

        let message = format!("{err:#}");
        assert!(message.contains("default seccomp gap audit"));
        assert!(message.contains("audit:POLICY_JSON:TRACE_JSONL"));
        assert!(!message.contains("--seccomp=off"));
    }

    #[test]
    fn default_gap_audit_fails_closed_with_invalid_default_policy() {
        let dir = tempfile::tempdir().expect("tempdir should exist");
        let policy = dir.path().join("invalid.json");
        fs::write(&policy, b"not json").expect("invalid policy fixture should be written");

        let err = resolve_normal_launch_seccomp_with(
            Some(SeccompMode::Audit(AuditMode::DefaultGap {
                trace_path: PathBuf::from("missing.jsonl"),
            })),
            || Some(policy),
        )
        .expect_err("invalid default policy should fail closed");

        let message = format!("{err:#}");
        assert!(message.contains("failed to load default loftd seccomp policy"));
        assert!(message.contains("default seccomp gap audit"));
        assert!(message.contains("audit:POLICY_JSON:TRACE_JSONL"));
    }

    #[test]
    fn explicit_audit_modes_bypass_default_policy_lookup() {
        let full = resolve_normal_launch_seccomp_with(
            Some(SeccompMode::Audit(AuditMode::Full {
                trace_path: PathBuf::from("trace.jsonl"),
            })),
            || panic!("full audit must not resolve the packaged default policy"),
        )
        .expect("full audit should resolve");
        assert_eq!(
            full,
            SeccompMode::Audit(AuditMode::Full {
                trace_path: PathBuf::from("trace.jsonl"),
            })
        );

        let gap = resolve_normal_launch_seccomp_with(
            Some(SeccompMode::Audit(AuditMode::Gap {
                baseline_policy_path: PathBuf::from("baseline.json"),
                trace_path: PathBuf::from("missing.jsonl"),
            })),
            || panic!("explicit gap audit must not resolve the packaged default policy"),
        )
        .expect("explicit gap audit should resolve");
        assert_eq!(
            gap,
            SeccompMode::Audit(AuditMode::Gap {
                baseline_policy_path: PathBuf::from("baseline.json"),
                trace_path: PathBuf::from("missing.jsonl"),
            })
        );
    }

    #[test]
    fn explicit_normal_launch_seccomp_off_bypasses_default_policy_lookup() {
        let seccomp = resolve_normal_launch_seccomp_with(Some(SeccompMode::Off), || {
            panic!("explicit seccomp mode should not resolve the packaged default policy")
        })
        .expect("explicit off should resolve");

        assert_eq!(seccomp, SeccompMode::Off);
    }

    #[test]
    fn omitted_normal_launch_seccomp_fails_closed_without_default_policy() {
        let err = resolve_normal_launch_seccomp_with(None, || None)
            .expect_err("missing default policy should fail closed");

        let message = format!("{err:#}");
        assert!(message.contains("default seccomp policy"));
        assert!(message.contains("--seccomp=off"));
    }

    #[test]
    fn plan_prepares_existing_agentbox_style_bind_mounts_without_codex_config() {
        let dir = tempfile::tempdir().expect("tempdir should exist");
        let workspace = dir.path().join("project");
        let home = dir.path().join("home");
        fs::create_dir_all(&workspace).expect("workspace should exist");

        let plan = LaunchPlan::from_env_values(
            runtime_options(),
            workspace.clone(),
            Some(dir.path().join("state").as_path()),
            Some(dir.path().join("config").as_path()),
            Some(home.as_path()),
        )
        .expect("plan should build");

        let mount = |target: &str| {
            plan.bind_mounts
                .iter()
                .find(|mount| mount.target == target)
                .expect("mount should exist")
        };
        assert_eq!(plan.bind_mounts.len(), 6);
        assert_eq!(mount("/workspace").source, workspace);
        assert_eq!(mount("/home/dev/.codex").source, home.join(".codex"));
        assert_eq!(mount("/home/dev/.omp").source, home.join(".omp"));
        assert_eq!(mount("/home/dev/.pi").source, home.join(".pi"));
        assert_eq!(
            mount("/home/dev/.cargo").source,
            plan.state_layout.root_dir().join("cargo")
        );
        assert_eq!(
            mount("/home/dev/.cache/sccache").source,
            plan.state_layout.sccache_dir()
        );
        assert!(home.join(".codex").is_dir());
        assert!(home.join(".omp").is_dir());
        assert!(home.join(".pi").is_dir());
        assert!(plan.state_layout.root_dir().join("cargo").is_dir());
        assert!(!plan.state_layout.root_dir().join("containers").exists());
        assert_eq!(
            fs::metadata(plan.state_layout.sccache_dir())
                .expect("sccache metadata")
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert!(
            !plan
                .bind_mounts
                .iter()
                .any(|mount| mount.target.contains(".config/codex")
                    || mount.source.to_string_lossy().contains(".config/codex"))
        );
    }

    #[test]
    fn plan_requires_home_for_codex_and_pi_mounts() {
        let dir = tempfile::tempdir().expect("tempdir should exist");
        let err = LaunchPlan::from_env_values(
            runtime_options(),
            PathBuf::from("/tmp/project"),
            Some(dir.path().join("state").as_path()),
            Some(dir.path().join("config").as_path()),
            None,
        )
        .expect_err("HOME should be required");

        assert!(format!("{err:#}").contains("HOME is not set"));
    }

    #[test]
    fn debug_compatibility_sets_effective_log_level_in_plan() {
        let dir = tempfile::tempdir().expect("tempdir should exist");
        let mut options = runtime_options();
        options.debug = true;
        options.log_settings = LogSettings::resolve(None, true, None);

        let plan = LaunchPlan::from_env_values(
            options,
            PathBuf::from("/tmp/project"),
            Some(dir.path().join("state").as_path()),
            Some(dir.path().join("config").as_path()),
            Some(dir.path().join("home").as_path()),
        )
        .expect("plan should build");

        assert_eq!(plan.log_level, LogLevel::Debug);
        assert!(plan.debug);
    }

    #[test]
    fn pull_latest_uses_canonical_refresh_image_selection() {
        let dir = tempfile::tempdir().expect("tempdir should exist");
        let mut options = runtime_options();
        options.pull_latest = true;

        let plan = LaunchPlan::from_env_values(
            options,
            PathBuf::from("/tmp/project"),
            Some(dir.path().join("state").as_path()),
            Some(dir.path().join("config").as_path()),
            Some(dir.path().join("home").as_path()),
        )
        .expect("plan should build");

        assert_eq!(plan.image_selection, ImageSelection::CanonicalWithRefresh);
        assert_eq!(
            plan.image_selection.selected_reference(),
            DEFAULT_FALLBACK_IMAGE
        );
    }

    #[test]
    fn explicit_image_is_preserved_as_image_selection() {
        let dir = tempfile::tempdir().expect("tempdir should exist");
        let mut options = runtime_options();
        options.image = Some("example/loftd:dev".to_owned());

        let plan = LaunchPlan::from_env_values(
            options,
            PathBuf::from("/tmp/project"),
            Some(dir.path().join("state").as_path()),
            Some(dir.path().join("config").as_path()),
            Some(dir.path().join("home").as_path()),
        )
        .expect("plan should build");

        assert_eq!(
            plan.image_selection,
            ImageSelection::Explicit {
                reference: "example/loftd:dev".to_owned()
            }
        );
        assert_eq!(
            plan.image_selection.selected_reference(),
            "example/loftd:dev"
        );
    }

    #[test]
    fn config_backend_and_state_location_are_loaded_into_plan() {
        let dir = tempfile::tempdir().expect("tempdir should exist");
        let config_home = dir.path().join("config");
        let state_home = dir.path().join("ignored-state");
        let home = dir.path().join("home");
        let configured_state_root = dir.path().join("configured-state");
        fs::create_dir_all(config_home.join("loftd")).expect("config dir should exist");
        fs::write(
            config_home.join("loftd").join("loftd.toml"),
            format!(
                "[state]\nlocation = \"{}\"\n\n[task-rootfs]\nbackend = \"fuse-overlay\"\n",
                configured_state_root.display()
            ),
        )
        .expect("config should be written");

        let plan = LaunchPlan::from_env_values(
            runtime_options(),
            PathBuf::from("/tmp/project"),
            Some(&state_home),
            Some(&config_home),
            Some(&home),
        )
        .expect("plan should build");

        assert_eq!(plan.task_rootfs_backend, TaskRootfsBackend::FuseOverlay);
        assert_eq!(
            plan.state_layout.root_dir(),
            configured_state_root.join("loftd").join("project")
        );
        assert_eq!(
            plan.config_diagnostics.config_path,
            config_home.join("loftd").join("loftd.toml")
        );
        assert!(plan.config_diagnostics.config_loaded);
    }

    #[test]
    fn cli_backend_overrides_config_backend() {
        let dir = tempfile::tempdir().expect("tempdir should exist");
        let config_home = dir.path().join("config");
        fs::create_dir_all(config_home.join("loftd")).expect("config dir should exist");
        fs::write(
            config_home.join("loftd").join("loftd.toml"),
            "[task-rootfs]\nbackend = \"fuse-overlay\"\n",
        )
        .expect("config should be written");
        let mut options = runtime_options();
        options.rootfs_backend = Some(TaskRootfsBackend::BtrfsSnapshot);

        let plan = LaunchPlan::from_env_values(
            options,
            PathBuf::from("/tmp/project"),
            Some(dir.path().join("state").as_path()),
            Some(config_home.as_path()),
            Some(dir.path().join("home").as_path()),
        )
        .expect("plan should build");

        assert_eq!(plan.task_rootfs_backend, TaskRootfsBackend::BtrfsSnapshot);
    }

    #[test]
    fn default_raw_disk_container_store_does_not_add_container_bind_mount() {
        let dir = tempfile::tempdir().expect("tempdir should exist");

        let plan = LaunchPlan::from_env_values(
            runtime_options(),
            PathBuf::from("/tmp/project"),
            Some(dir.path().join("state").as_path()),
            Some(dir.path().join("config").as_path()),
            Some(dir.path().join("home").as_path()),
        )
        .expect("plan should build");

        assert_eq!(plan.container_store_backend, ContainerStoreBackend::RawDisk);
        assert!(
            !plan
                .bind_mounts
                .iter()
                .any(|mount| mount.target == "/home/dev/.local/share/containers")
        );
    }

    #[test]
    fn launch_plan_carries_shell_and_debug_options() {
        let dir = tempfile::tempdir().expect("tempdir should exist");
        let mut options = runtime_options();
        options.guest_init = Some(Path::new("./loftd-guest-init").to_path_buf());
        options.mem_gib = Some(8);
        options.guest_command = vec!["bash".to_owned(), "-lc".to_owned(), "echo ok".to_owned()];
        options.debug = true;
        options.log_settings = LogSettings::resolve(None, true, None);
        options.profile = true;
        options.root = true;
        options.daemon = true;
        options.preserve_debug = true;

        let plan = LaunchPlan::from_env_values(
            options,
            PathBuf::from("/tmp/project"),
            Some(dir.path().join("state").as_path()),
            Some(dir.path().join("config").as_path()),
            Some(dir.path().join("home").as_path()),
        )
        .expect("plan should build");

        assert_eq!(
            plan.guest_init,
            Some(Path::new("./loftd-guest-init").to_path_buf())
        );
        assert_eq!(plan.mem_gib, Some(8));
        assert_eq!(plan.network_mode, NetworkMode::Tsi);
        assert_eq!(plan.guest_command, ["bash", "-lc", "echo ok"]);
        assert!(plan.debug);
        assert!(plan.profile);
        assert!(plan.root);
        assert!(plan.daemon);
        assert!(plan.preserve_debug);
    }

    #[test]
    fn launch_plan_carries_passt_network_mode() {
        let dir = tempfile::tempdir().expect("tempdir should exist");
        let mut options = runtime_options();
        options.network_mode = NetworkMode::Passt;

        let plan = LaunchPlan::from_env_values(
            options,
            PathBuf::from("/tmp/project"),
            Some(dir.path().join("state").as_path()),
            Some(dir.path().join("config").as_path()),
            Some(dir.path().join("home").as_path()),
        )
        .expect("plan should build");

        assert_eq!(plan.network_mode, NetworkMode::Passt);
    }

    #[test]
    fn launch_plan_carries_publish_specs() {
        let dir = tempfile::tempdir().expect("tempdir should exist");
        let mut options = runtime_options();
        options.publish = vec!["8080:80".to_owned(), "8443:443".to_owned()];

        let plan = LaunchPlan::from_env_values(
            options,
            PathBuf::from("/tmp/project"),
            Some(dir.path().join("state").as_path()),
            Some(dir.path().join("config").as_path()),
            Some(dir.path().join("home").as_path()),
        )
        .expect("plan should build");

        assert_eq!(plan.publish, ["8080:80", "8443:443"]);
    }

    #[test]
    fn launch_plan_appends_user_directory_and_file_volumes() {
        let dir = tempfile::tempdir().expect("tempdir should exist");
        let workspace = dir.path().join("project");
        let home = dir.path().join("home");
        let source_dir = dir.path().join("host-dir");
        let source_file = workspace.join("host-file");
        fs::create_dir_all(&workspace).expect("workspace should exist");
        fs::create_dir_all(&source_dir).expect("source dir");
        fs::write(&source_file, "data").expect("source file");
        let mut options = runtime_options();
        options.volumes = vec![
            VolumeSpec {
                source: source_dir.clone(),
                target: "/guest/dir".to_owned(),
                read_only: false,
            },
            VolumeSpec {
                source: PathBuf::from("host-file"),
                target: "/guest/file".to_owned(),
                read_only: true,
            },
        ];

        let plan = LaunchPlan::from_env_values(
            options,
            workspace.clone(),
            Some(dir.path().join("state").as_path()),
            Some(dir.path().join("config").as_path()),
            Some(home.as_path()),
        )
        .expect("plan should build");

        let dir_mount = plan
            .bind_mounts
            .iter()
            .find(|mount| mount.target == "/guest/dir")
            .expect("dir volume mount");
        assert_eq!(dir_mount.source, source_dir);
        assert_eq!(dir_mount.source_kind, BindMountSourceKind::Directory);
        assert!(!dir_mount.read_only);

        let file_mount = plan
            .bind_mounts
            .iter()
            .find(|mount| mount.target == "/guest/file")
            .expect("file volume mount");
        assert_eq!(file_mount.source, source_file);
        assert_eq!(file_mount.source_kind, BindMountSourceKind::File);
        assert!(file_mount.read_only);
    }

    #[test]
    fn launch_plan_rejects_user_volume_reserved_or_duplicate_targets() {
        let dir = tempfile::tempdir().expect("tempdir should exist");
        let workspace = dir.path().join("project");
        let home = dir.path().join("home");
        let source = dir.path().join("host-dir");
        fs::create_dir_all(&workspace).expect("workspace should exist");
        fs::create_dir_all(&source).expect("source dir");

        for target in [
            "/",
            "/.",
            "/nix",
            "/nix//",
            "/workspace",
            "/workspace/",
            "/workspace/.",
            "/home/dev/.codex",
            "/home/dev/.codex/./",
        ] {
            let mut options = runtime_options();
            options.volumes = vec![VolumeSpec {
                source: source.clone(),
                target: target.to_owned(),
                read_only: false,
            }];
            let err = LaunchPlan::from_env_values(
                options,
                workspace.clone(),
                Some(dir.path().join("state").as_path()),
                Some(dir.path().join("config").as_path()),
                Some(home.as_path()),
            )
            .expect_err("reserved or duplicate target should fail");

            let error = format!("{err:#}");
            assert!(
                error.contains("reserved")
                    || error.contains("duplicated")
                    || error.contains("must not be /"),
                "unexpected error for {target}: {error}"
            );
        }
    }

    #[test]
    fn launch_plan_rejects_missing_user_volume_source() {
        let dir = tempfile::tempdir().expect("tempdir should exist");
        let workspace = dir.path().join("project");
        fs::create_dir_all(&workspace).expect("workspace should exist");
        let mut options = runtime_options();
        options.volumes = vec![VolumeSpec {
            source: PathBuf::from("missing"),
            target: "/guest/missing".to_owned(),
            read_only: false,
        }];

        let err = LaunchPlan::from_env_values(
            options,
            workspace,
            Some(dir.path().join("state").as_path()),
            Some(dir.path().join("config").as_path()),
            Some(dir.path().join("home").as_path()),
        )
        .expect_err("missing source should fail");

        assert!(format!("{err:#}").contains("failed to inspect volume source"));
    }
}
