use anyhow::Result;
use std::env;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use crate::cli::RuntimeOptions;
use crate::config;
use crate::logging::LogLevel;
use crate::naming::derive_workspace_slug;
use crate::runtime::launch_config::{
    BindMount, CARGO_TAG, CARGO_TARGET, CODEX_TAG, CODEX_TARGET, NetworkMode, PI_TAG, PI_TARGET,
    SCCACHE_TAG, SCCACHE_TARGET, WORKSPACE_TAG, WORKSPACE_TARGET, validate_mounts,
};
use crate::state::{self, StateLayout};
use crate::task_rootfs::TaskRootfsBackend;
use crate::{DEFAULT_FALLBACK_IMAGE, DEFAULT_IMAGE};

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
    pub(crate) guest_init: Option<PathBuf>,
    pub(crate) mem_gib: Option<u32>,
    pub(crate) network_mode: NetworkMode,
    pub(crate) guest_command: Vec<String>,
    pub(crate) debug: bool,
    pub(crate) log_level: LogLevel,
    pub(crate) profile: bool,
    pub(crate) root: bool,
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
            anyhow::anyhow!("HOME is not set; loftd cannot prepare .codex and .pi bind mounts")
        })?;
        let bind_mounts = prepare_bind_mounts(&workspace_dir, home_dir, &state_layout)?;
        let task_rootfs_backend = options
            .rootfs_backend
            .or_else(|| config.task_rootfs_backend())
            .unwrap_or(TaskRootfsBackend::DEFAULT);

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
            guest_init: options.guest_init,
            mem_gib: options.mem_gib,
            network_mode: options.network_mode,
            guest_command: options.guest_command,
            debug: options.log_settings.level.enables_debug(),
            log_level: options.log_settings.level,
            profile: options.profile,
            root: options.root,
            preserve_debug: options.preserve_debug,
            config_diagnostics: ConfigDiagnostics {
                config_path: config.path().to_path_buf(),
                config_loaded: config.loaded(),
            },
        })
    }
}

fn derive_runtime_hostname(workspace_slug: &str) -> String {
    format!("loftd-{workspace_slug}")
}

fn prepare_bind_mounts(
    workspace_dir: &Path,
    home_dir: &Path,
    state_layout: &StateLayout,
) -> Result<Vec<BindMount>> {
    let codex_dir = home_dir.join(".codex");
    let pi_dir = home_dir.join(".pi");
    let cargo_dir = state_layout.root_dir().join("cargo");
    let sccache_dir = state_layout.sccache_dir();

    fs::create_dir_all(&codex_dir)
        .map_err(|err| anyhow::anyhow!("failed to create '{}': {err}", codex_dir.display()))?;
    fs::create_dir_all(&pi_dir)
        .map_err(|err| anyhow::anyhow!("failed to create '{}': {err}", pi_dir.display()))?;
    fs::create_dir_all(&cargo_dir)
        .map_err(|err| anyhow::anyhow!("failed to create '{}': {err}", cargo_dir.display()))?;
    fs::create_dir_all(&sccache_dir)
        .map_err(|err| anyhow::anyhow!("failed to create '{}': {err}", sccache_dir.display()))?;
    fs::set_permissions(&sccache_dir, fs::Permissions::from_mode(0o700))
        .map_err(|err| anyhow::anyhow!("failed to chmod 700 '{}': {err}", sccache_dir.display()))?;

    let mounts = vec![
        bind_mount(workspace_dir, WORKSPACE_TAG, WORKSPACE_TARGET),
        bind_mount(&codex_dir, CODEX_TAG, CODEX_TARGET),
        bind_mount(&pi_dir, PI_TAG, PI_TARGET),
        bind_mount(&cargo_dir, CARGO_TAG, CARGO_TARGET),
        bind_mount(&sccache_dir, SCCACHE_TAG, SCCACHE_TARGET),
    ];
    validate_mounts(&mounts)?;
    Ok(mounts)
}

fn bind_mount(source: &Path, tag: &str, target: &str) -> BindMount {
    BindMount {
        source: source.to_path_buf(),
        tag: tag.to_owned(),
        target: target.to_owned(),
    }
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

    use crate::cli::RuntimeOptions;
    use crate::logging::{LogLevel, LogSettings};
    use crate::runtime::launch_config::NetworkMode;
    use crate::runtime::launch_plan::{ImageSelection, LaunchPlan};
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
            rootfs_backend: None,
            guest_init: None,
            preserve_debug: false,
            mem_gib: None,
            network_mode: NetworkMode::Tsi,
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
        assert_eq!(plan.log_level, LogLevel::Off);
        assert!(!plan.debug);
        assert!(!plan.config_diagnostics.config_loaded);
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
        assert_eq!(plan.bind_mounts.len(), 5);
        assert_eq!(mount("/workspace").source, workspace);
        assert_eq!(mount("/home/dev/.codex").source, home.join(".codex"));
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
        assert!(home.join(".pi").is_dir());
        assert!(plan.state_layout.root_dir().join("cargo").is_dir());
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
}
