pub(crate) mod ffi;
pub(crate) mod guest_init;
pub(crate) mod image_cache;
pub(crate) mod launch;
pub(crate) mod persistent_disks;
pub(crate) mod storage;
pub(crate) mod supervisor;

use anyhow::{Context, Result};
use std::path::Path;
use std::process::ExitCode;

use crate::cli::{CommonOptions, MicrovmOptions};
use crate::naming::derive_task_container_name;
use crate::state::resolve_state_layout;

use self::guest_init::resolve_guest_init;
use self::image_cache::{BuildahRunner, HostBuildahRunner, ImageCache, ImageReference};
use self::launch::{MicrovmLaunchConfig, MicrovmLaunchSpec, resolve_cpu_count};
use self::persistent_disks::{HostMicrovmPersistentDiskPreparer, MicrovmPersistentDiskPreparer};
use self::storage::{HostStorageProbe, StorageManager, StorageProbe};
use self::supervisor::{HostMicrovmSupervisor, MicrovmSupervisor};

struct MicrovmRunRequest<'a> {
    common: CommonOptions,
    options: MicrovmOptions,
    state_layout: &'a crate::state::StateLayout,
    task_id: &'a str,
    workspace_source: &'a Path,
}

struct MicrovmRunDeps<'a, B, S, M, D> {
    buildah: &'a B,
    storage_probe: &'a S,
    supervisor: &'a M,
    disk_preparer: &'a D,
}

pub(crate) fn run(common: CommonOptions, options: MicrovmOptions) -> Result<ExitCode> {
    let cwd = std::env::current_dir()?
        .canonicalize()
        .context("failed to canonicalize current directory")?;
    let state_layout = resolve_state_layout(&cwd)?;
    let task_id = derive_task_container_name(&cwd);
    run_with_layout(common, options, &state_layout, &task_id, &cwd)
}

fn run_with_layout(
    common: CommonOptions,
    options: MicrovmOptions,
    state_layout: &crate::state::StateLayout,
    task_id: &str,
    workspace_source: &Path,
) -> Result<ExitCode> {
    run_with_deps(
        MicrovmRunRequest {
            common,
            options,
            state_layout,
            task_id,
            workspace_source,
        },
        MicrovmRunDeps {
            buildah: &HostBuildahRunner,
            storage_probe: &HostStorageProbe,
            supervisor: &HostMicrovmSupervisor,
            disk_preparer: &HostMicrovmPersistentDiskPreparer,
        },
    )
}

fn run_with_deps(
    request: MicrovmRunRequest<'_>,
    deps: MicrovmRunDeps<
        '_,
        impl BuildahRunner,
        impl StorageProbe,
        impl MicrovmSupervisor,
        impl MicrovmPersistentDiskPreparer,
    >,
) -> Result<ExitCode> {
    if request.common.pull_latest {
        anyhow::bail!(pull_latest_not_supported_message());
    }

    let reference = ImageReference::from_cli(request.common.image.as_deref());
    let cache = ImageCache::new(request.state_layout.microvm_image_cache_dir());
    let entry = cache
        .ensure(reference, deps.buildah)
        .context("microvm image cache ingestion failed")?;
    let backend = StorageManager::select_backend(request.options.storage, deps.storage_probe)
        .context("microvm storage backend selection failed")?;
    let storage = StorageManager::new(request.state_layout.root_dir().to_path_buf());
    let handle = storage
        .materialize(
            &entry,
            backend,
            request.task_id,
            request.options.preserve_debug,
        )
        .context("microvm task rootfs materialization failed")?;
    let run_result = (|| {
        let task_state_dir = handle
            .root
            .parent()
            .ok_or_else(|| anyhow::anyhow!("microvm task rootfs has no parent state dir"))?
            .to_path_buf();
        let guest_init = resolve_guest_init(&handle.root, request.options.guest_init.as_deref())
            .context("microvm guest-init resolution failed")?;
        let disks = deps
            .disk_preparer
            .prepare(request.state_layout.root_dir())
            .context("microvm persistent disk preparation failed")?;
        let (host_uid, host_gid) = current_host_ids();
        let config = MicrovmLaunchConfig::build_for_task(MicrovmLaunchSpec {
            task_rootfs: &handle.root,
            workspace_source: request.workspace_source,
            guest_init_exec: &guest_init.guest_exec_path,
            common: request.common,
            options: request.options.clone(),
            host_uid,
            host_gid,
            vcpus: resolve_cpu_count()?,
            disks: disks.attachments(),
            extra_env: disks.env_pairs(),
        })
        .context("microvm launch config build failed")?;

        deps.supervisor
            .run(&config, &task_state_dir)
            .with_context(|| {
                launch_failure_context(
                    request.options.preserve_debug,
                    &handle.root,
                    &task_state_dir,
                )
            })
    })();
    let cleanup_result = handle.cleanup();
    let status = match (run_result, cleanup_result) {
        (Ok(status), Ok(_)) => status,
        (Ok(_), Err(cleanup_err)) => return Err(cleanup_err),
        (Err(run_err), Ok(_)) => return Err(run_err),
        (Err(run_err), Err(cleanup_err)) => {
            return Err(cleanup_err.context(format!(
                "microvm task rootfs cleanup failed after run error: {run_err:#}"
            )));
        }
    };
    Ok(status.exit_code())
}

fn launch_failure_context(
    preserve_debug: bool,
    task_rootfs: &Path,
    task_state_dir: &Path,
) -> String {
    let launch_config = task_state_dir.join("launch.conf");
    if preserve_debug {
        return format!(
            "microvm launch failed; preserved microvm debug state: task_rootfs='{}', task_state_dir='{}', launch_config='{}'",
            task_rootfs.display(),
            task_state_dir.display(),
            launch_config.display()
        );
    }
    format!(
        "microvm launch failed; task debug state was cleaned unless cleanup reported another error; rerun with --preserve-debug to keep task_rootfs='{}' and launch_config='{}'",
        task_rootfs.display(),
        launch_config.display()
    )
}

pub(crate) fn run_helper_from_path(config_path: &Path) -> Result<()> {
    supervisor::run_helper(config_path)
}

#[cfg(test)]
pub(crate) fn boot_pending_message() -> &'static str {
    "experimental microvm direct libkrun boot is enabled"
}

pub(crate) fn pull_latest_not_supported_message() -> &'static str {
    "agentbox --pull-latest microvm is not supported yet; experimental microvm image refresh must use a future Buildah-backed path, not Podman"
}

fn current_host_ids() -> (u32, u32) {
    (unsafe { libc::getuid() }, unsafe { libc::getgid() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::MicrovmStoragePolicy;
    use crate::runtime::microvm::image_cache::{ImageDigest, ImageReference};
    use crate::runtime::microvm::persistent_disks::test_support::FakePersistentDiskPreparer;
    use crate::runtime::microvm::supervisor::MicrovmChildStatus;
    use std::cell::RefCell;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::rc::Rc;

    struct NoBuildah;

    impl BuildahRunner for NoBuildah {
        fn ingest(&self, _reference: &ImageReference, _cache_root: &Path) -> Result<ImageDigest> {
            anyhow::bail!("buildah should not be called")
        }
    }

    struct Probe;

    impl StorageProbe for Probe {
        fn btrfs_available(&self) -> bool {
            false
        }
        fn fuse_overlay_available(&self) -> bool {
            true
        }
    }

    struct MissingStorageProbe;

    impl StorageProbe for MissingStorageProbe {
        fn btrfs_available(&self) -> bool {
            false
        }

        fn fuse_overlay_available(&self) -> bool {
            false
        }
    }

    #[derive(Clone)]
    struct FakeSupervisor {
        calls: Rc<RefCell<Vec<MicrovmLaunchConfig>>>,
        status: MicrovmChildStatus,
    }

    impl MicrovmSupervisor for FakeSupervisor {
        fn run(
            &self,
            config: &MicrovmLaunchConfig,
            _task_state_dir: &Path,
        ) -> Result<MicrovmChildStatus> {
            self.calls.borrow_mut().push(config.clone());
            Ok(self.status)
        }
    }

    #[derive(Clone)]
    struct FailingSupervisor;

    impl MicrovmSupervisor for FailingSupervisor {
        fn run(
            &self,
            _config: &MicrovmLaunchConfig,
            _task_state_dir: &Path,
        ) -> Result<MicrovmChildStatus> {
            anyhow::bail!("fake supervisor failed")
        }
    }

    fn write_cached_guest_init(state_layout: &crate::state::StateLayout) {
        let cache = ImageCache::new(state_layout.microvm_image_cache_dir());
        let digest = ImageDigest::parse("sha256:abc123").expect("digest should parse");
        let entry = cache.entry_path(&digest);
        let guest_init = entry.join("rootfs/nix/store/hash-agentbox/bin/agentbox-guest-init");
        fs::create_dir_all(guest_init.parent().unwrap()).expect("guest init parent");
        fs::write(&guest_init, "#!/bin/sh\n").expect("guest init write");
        fs::set_permissions(&guest_init, fs::Permissions::from_mode(0o755))
            .expect("guest init mode");
        fs::write(entry.join("agentbox-compatible"), "agentbox\n").expect("compat marker");
        cache
            .record_ref_digest(
                &ImageReference::from_cli(Some("ghcr.io/example/agentbox@sha256:abc123")),
                &digest,
            )
            .expect("ref should be recorded");
    }

    fn write_cached_compatible_rootfs_without_guest_init(state_layout: &crate::state::StateLayout) {
        let cache = ImageCache::new(state_layout.microvm_image_cache_dir());
        let digest = ImageDigest::parse("sha256:abc123").expect("digest should parse");
        let entry = cache.entry_path(&digest);
        fs::create_dir_all(entry.join("rootfs/nix/store/hash-agentbox/bin"))
            .expect("rootfs should exist");
        fs::write(entry.join("agentbox-compatible"), "agentbox\n").expect("compat marker");
        cache
            .record_ref_digest(
                &ImageReference::from_cli(Some("ghcr.io/example/agentbox@sha256:abc123")),
                &digest,
            )
            .expect("ref should be recorded");
    }

    fn ok_disk_preparer() -> FakePersistentDiskPreparer {
        FakePersistentDiskPreparer::ok(Rc::new(RefCell::new(Vec::new())))
    }

    #[test]
    fn run_with_deps_supervises_direct_launch_and_cleans_task_rootfs() {
        let temp = tempfile::tempdir().expect("tempdir");
        let state_layout = crate::state::StateLayout::for_test(temp.path().join("state"));
        write_cached_guest_init(&state_layout);
        let calls = Rc::new(RefCell::new(Vec::new()));
        let supervisor = FakeSupervisor {
            calls: calls.clone(),
            status: MicrovmChildStatus::exited(42),
        };
        let disk_preparer = ok_disk_preparer();

        let code = run_with_deps(
            MicrovmRunRequest {
                common: CommonOptions {
                    image: Some("ghcr.io/example/agentbox@sha256:abc123".to_owned()),
                    pull_latest: false,
                    debug: false,
                    profile: false,
                    root: false,
                },
                options: MicrovmOptions {
                    storage: MicrovmStoragePolicy::FuseOverlay,
                    guest_init: None,
                    preserve_debug: false,
                    mem_gib: Some(2),
                },
                state_layout: &state_layout,
                task_id: "task-one",
                workspace_source: temp.path(),
            },
            MicrovmRunDeps {
                buildah: &NoBuildah,
                storage_probe: &Probe,
                supervisor: &supervisor,
                disk_preparer: &disk_preparer,
            },
        )
        .expect("microvm run should succeed");

        assert_eq!(code, ExitCode::from(42));
        assert_eq!(calls.borrow().len(), 1);
        assert!(
            !state_layout
                .root_dir()
                .join("microvm-tasks/task-one/rootfs-fuse-overlay")
                .exists()
        );
    }

    #[test]
    fn preserve_debug_keeps_task_rootfs_after_supervisor() {
        let temp = tempfile::tempdir().expect("tempdir");
        let state_layout = crate::state::StateLayout::for_test(temp.path().join("state"));
        write_cached_guest_init(&state_layout);
        let supervisor = FakeSupervisor {
            calls: Rc::new(RefCell::new(Vec::new())),
            status: MicrovmChildStatus::exited(0),
        };
        let disk_preparer = ok_disk_preparer();

        run_with_deps(
            MicrovmRunRequest {
                common: CommonOptions {
                    image: Some("ghcr.io/example/agentbox@sha256:abc123".to_owned()),
                    pull_latest: false,
                    debug: false,
                    profile: false,
                    root: false,
                },
                options: MicrovmOptions {
                    storage: MicrovmStoragePolicy::FuseOverlay,
                    guest_init: None,
                    preserve_debug: true,
                    mem_gib: Some(2),
                },
                state_layout: &state_layout,
                task_id: "task-one",
                workspace_source: temp.path(),
            },
            MicrovmRunDeps {
                buildah: &NoBuildah,
                storage_probe: &Probe,
                supervisor: &supervisor,
                disk_preparer: &disk_preparer,
            },
        )
        .expect("microvm run should succeed");

        assert!(
            state_layout
                .root_dir()
                .join("microvm-tasks/task-one/rootfs-fuse-overlay")
                .exists()
        );
    }
    #[test]
    fn run_with_deps_prepares_persistent_dev_cache_disks_before_supervisor() {
        let temp = tempfile::tempdir().expect("tempdir");
        let state_layout = crate::state::StateLayout::for_test(temp.path().join("state"));
        write_cached_guest_init(&state_layout);
        let calls = Rc::new(RefCell::new(Vec::new()));
        let supervisor = FakeSupervisor {
            calls: calls.clone(),
            status: MicrovmChildStatus::exited(0),
        };
        let disk_calls = Rc::new(RefCell::new(Vec::new()));
        let disk_preparer = FakePersistentDiskPreparer::ok(disk_calls.clone());

        run_with_deps(
            MicrovmRunRequest {
                common: CommonOptions {
                    image: Some("ghcr.io/example/agentbox@sha256:abc123".to_owned()),
                    pull_latest: false,
                    debug: false,
                    profile: false,
                    root: false,
                },
                options: MicrovmOptions {
                    storage: MicrovmStoragePolicy::FuseOverlay,
                    guest_init: None,
                    preserve_debug: false,
                    mem_gib: Some(2),
                },
                state_layout: &state_layout,
                task_id: "task-one",
                workspace_source: temp.path(),
            },
            MicrovmRunDeps {
                buildah: &NoBuildah,
                storage_probe: &Probe,
                supervisor: &supervisor,
                disk_preparer: &disk_preparer,
            },
        )
        .expect("microvm run should succeed");

        assert_eq!(
            disk_calls.borrow().as_slice(),
            &[state_layout.root_dir().to_path_buf()]
        );
        let config = calls.borrow().first().expect("supervisor config").clone();
        assert_eq!(config.disks.len(), 2);
        assert!(config.disks.iter().any(|disk| disk.id == "agentbox-nix"));
        assert!(
            config
                .disks
                .iter()
                .any(|disk| disk.id == "agentbox-containers")
        );
        assert!(config.env_contains("AGENTBOX_LIBKRUN_NIX_OVERLAY", "1"));
        assert!(config.env_contains("AGENTBOX_LIBKRUN_CONTAINERS_STORAGE", "1"));
    }

    #[test]
    fn disk_prep_failure_cleans_task_rootfs_unless_preserve_debug() {
        let temp = tempfile::tempdir().expect("tempdir");
        let state_layout = crate::state::StateLayout::for_test(temp.path().join("state"));
        write_cached_guest_init(&state_layout);
        let supervisor = FakeSupervisor {
            calls: Rc::new(RefCell::new(Vec::new())),
            status: MicrovmChildStatus::exited(0),
        };
        let disk_preparer = FakePersistentDiskPreparer::failing(Rc::new(RefCell::new(Vec::new())));

        let err = run_with_deps(
            MicrovmRunRequest {
                common: CommonOptions {
                    image: Some("ghcr.io/example/agentbox@sha256:abc123".to_owned()),
                    pull_latest: false,
                    debug: false,
                    profile: false,
                    root: false,
                },
                options: MicrovmOptions {
                    storage: MicrovmStoragePolicy::FuseOverlay,
                    guest_init: None,
                    preserve_debug: false,
                    mem_gib: Some(2),
                },
                state_layout: &state_layout,
                task_id: "task-one",
                workspace_source: temp.path(),
            },
            MicrovmRunDeps {
                buildah: &NoBuildah,
                storage_probe: &Probe,
                supervisor: &supervisor,
                disk_preparer: &disk_preparer,
            },
        )
        .expect_err("disk prep should fail");

        assert!(format!("{err:#}").contains("fake disk prep failed"));
        assert!(
            !state_layout
                .root_dir()
                .join("microvm-tasks/task-one/rootfs-fuse-overlay")
                .exists()
        );
    }

    #[test]
    fn preserve_debug_keeps_task_rootfs_after_disk_prep_failure() {
        let temp = tempfile::tempdir().expect("tempdir");
        let state_layout = crate::state::StateLayout::for_test(temp.path().join("state"));
        write_cached_guest_init(&state_layout);
        let supervisor = FakeSupervisor {
            calls: Rc::new(RefCell::new(Vec::new())),
            status: MicrovmChildStatus::exited(0),
        };
        let disk_preparer = FakePersistentDiskPreparer::failing(Rc::new(RefCell::new(Vec::new())));

        let err = run_with_deps(
            MicrovmRunRequest {
                common: CommonOptions {
                    image: Some("ghcr.io/example/agentbox@sha256:abc123".to_owned()),
                    pull_latest: false,
                    debug: false,
                    profile: false,
                    root: false,
                },
                options: MicrovmOptions {
                    storage: MicrovmStoragePolicy::FuseOverlay,
                    guest_init: None,
                    preserve_debug: true,
                    mem_gib: Some(2),
                },
                state_layout: &state_layout,
                task_id: "task-one",
                workspace_source: temp.path(),
            },
            MicrovmRunDeps {
                buildah: &NoBuildah,
                storage_probe: &Probe,
                supervisor: &supervisor,
                disk_preparer: &disk_preparer,
            },
        )
        .expect_err("disk prep should fail");

        assert!(format!("{err:#}").contains("fake disk prep failed"));
        assert!(
            state_layout
                .root_dir()
                .join("microvm-tasks/task-one/rootfs-fuse-overlay")
                .exists()
        );
    }

    #[test]
    fn preserve_debug_failure_reports_inspection_paths_after_materialization() {
        let temp = tempfile::tempdir().expect("tempdir");
        let state_layout = crate::state::StateLayout::for_test(temp.path().join("state"));
        write_cached_guest_init(&state_layout);
        let disk_preparer = ok_disk_preparer();

        let err = run_with_deps(
            MicrovmRunRequest {
                common: CommonOptions {
                    image: Some("ghcr.io/example/agentbox@sha256:abc123".to_owned()),
                    pull_latest: false,
                    debug: false,
                    profile: false,
                    root: false,
                },
                options: MicrovmOptions {
                    storage: MicrovmStoragePolicy::FuseOverlay,
                    guest_init: None,
                    preserve_debug: true,
                    mem_gib: Some(2),
                },
                state_layout: &state_layout,
                task_id: "task-one",
                workspace_source: temp.path(),
            },
            MicrovmRunDeps {
                buildah: &NoBuildah,
                storage_probe: &Probe,
                supervisor: &FailingSupervisor,
                disk_preparer: &disk_preparer,
            },
        )
        .expect_err("supervisor should fail");

        let message = format!("{err:#}");
        let task_dir = state_layout.root_dir().join("microvm-tasks/task-one");
        let rootfs = task_dir.join("rootfs-fuse-overlay");
        assert!(message.contains("preserved microvm debug state"));
        assert!(message.contains(&rootfs.display().to_string()));
        assert!(message.contains(&task_dir.join("launch.conf").display().to_string()));
        assert!(rootfs.exists());
    }

    #[test]
    fn run_with_deps_classifies_pre_launch_failure_phases() {
        let temp = tempfile::tempdir().expect("tempdir");
        let state_layout = crate::state::StateLayout::for_test(temp.path().join("state"));
        let supervisor = FakeSupervisor {
            calls: Rc::new(RefCell::new(Vec::new())),
            status: MicrovmChildStatus::exited(0),
        };
        let disk_preparer = ok_disk_preparer();

        let cache_err = run_with_deps(
            MicrovmRunRequest {
                common: CommonOptions {
                    image: Some("ghcr.io/example/agentbox:missing".to_owned()),
                    pull_latest: false,
                    debug: false,
                    profile: false,
                    root: false,
                },
                options: MicrovmOptions {
                    storage: MicrovmStoragePolicy::FuseOverlay,
                    guest_init: None,
                    preserve_debug: false,
                    mem_gib: Some(2),
                },
                state_layout: &state_layout,
                task_id: "cache-miss",
                workspace_source: temp.path(),
            },
            MicrovmRunDeps {
                buildah: &NoBuildah,
                storage_probe: &Probe,
                supervisor: &supervisor,
                disk_preparer: &disk_preparer,
            },
        )
        .expect_err("cache miss should fail before launch");
        assert!(format!("{cache_err:#}").contains("microvm image cache ingestion failed"));

        write_cached_compatible_rootfs_without_guest_init(&state_layout);
        let storage_err = run_with_deps(
            MicrovmRunRequest {
                common: CommonOptions {
                    image: Some("ghcr.io/example/agentbox@sha256:abc123".to_owned()),
                    pull_latest: false,
                    debug: false,
                    profile: false,
                    root: false,
                },
                options: MicrovmOptions {
                    storage: MicrovmStoragePolicy::Auto,
                    guest_init: None,
                    preserve_debug: false,
                    mem_gib: Some(2),
                },
                state_layout: &state_layout,
                task_id: "storage-missing",
                workspace_source: temp.path(),
            },
            MicrovmRunDeps {
                buildah: &NoBuildah,
                storage_probe: &MissingStorageProbe,
                supervisor: &supervisor,
                disk_preparer: &disk_preparer,
            },
        )
        .expect_err("missing storage helpers should fail before materialization");
        assert!(format!("{storage_err:#}").contains("microvm storage backend selection failed"));

        let guest_init_err = run_with_deps(
            MicrovmRunRequest {
                common: CommonOptions {
                    image: Some("ghcr.io/example/agentbox@sha256:abc123".to_owned()),
                    pull_latest: false,
                    debug: false,
                    profile: false,
                    root: false,
                },
                options: MicrovmOptions {
                    storage: MicrovmStoragePolicy::FuseOverlay,
                    guest_init: None,
                    preserve_debug: false,
                    mem_gib: Some(2),
                },
                state_layout: &state_layout,
                task_id: "guest-init-missing",
                workspace_source: temp.path(),
            },
            MicrovmRunDeps {
                buildah: &NoBuildah,
                storage_probe: &Probe,
                supervisor: &supervisor,
                disk_preparer: &disk_preparer,
            },
        )
        .expect_err("missing guest-init should fail before disk prep");
        assert!(format!("{guest_init_err:#}").contains("microvm guest-init resolution failed"));

        write_cached_guest_init(&state_layout);
        let disk_err = run_with_deps(
            MicrovmRunRequest {
                common: CommonOptions {
                    image: Some("ghcr.io/example/agentbox@sha256:abc123".to_owned()),
                    pull_latest: false,
                    debug: false,
                    profile: false,
                    root: false,
                },
                options: MicrovmOptions {
                    storage: MicrovmStoragePolicy::FuseOverlay,
                    guest_init: None,
                    preserve_debug: false,
                    mem_gib: Some(2),
                },
                state_layout: &state_layout,
                task_id: "disk-failure",
                workspace_source: temp.path(),
            },
            MicrovmRunDeps {
                buildah: &NoBuildah,
                storage_probe: &Probe,
                supervisor: &supervisor,
                disk_preparer: &FakePersistentDiskPreparer::failing(Rc::new(RefCell::new(
                    Vec::new(),
                ))),
            },
        )
        .expect_err("disk prep should fail before launch");
        assert!(format!("{disk_err:#}").contains("microvm persistent disk preparation failed"));

        let config_err = run_with_deps(
            MicrovmRunRequest {
                common: CommonOptions {
                    image: Some("ghcr.io/example/agentbox@sha256:abc123".to_owned()),
                    pull_latest: false,
                    debug: false,
                    profile: false,
                    root: false,
                },
                options: MicrovmOptions {
                    storage: MicrovmStoragePolicy::FuseOverlay,
                    guest_init: None,
                    preserve_debug: false,
                    mem_gib: Some(u32::MAX),
                },
                state_layout: &state_layout,
                task_id: "config-failure",
                workspace_source: temp.path(),
            },
            MicrovmRunDeps {
                buildah: &NoBuildah,
                storage_probe: &Probe,
                supervisor: &supervisor,
                disk_preparer: &disk_preparer,
            },
        )
        .expect_err("invalid launch config should fail before supervisor");
        assert!(format!("{config_err:#}").contains("microvm launch config build failed"));
    }

    #[test]
    fn microvm_direct_launch_modules_do_not_render_podman_or_oci_runtimes() {
        for (name, source) in [
            ("launch", include_str!("launch.rs")),
            ("supervisor", include_str!("supervisor.rs")),
            ("ffi", include_str!("ffi.rs")),
            ("guest_init", include_str!("guest_init.rs")),
            ("persistent_disks", include_str!("persistent_disks.rs")),
        ] {
            let lower = source.to_lowercase();
            let tokens = lower
                .split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '-'))
                .collect::<Vec<_>>();
            assert!(!lower.contains("podman run"), "{name} renders podman run");
            assert!(!tokens.contains(&"crun"), "{name} mentions crun");
            assert!(!tokens.contains(&"runc"), "{name} mentions runc");
        }
    }

    #[test]
    fn buildah_is_isolated_to_microvm_image_cache() {
        for (name, source) in [
            ("launch", include_str!("launch.rs")),
            ("supervisor", include_str!("supervisor.rs")),
            ("ffi", include_str!("ffi.rs")),
            ("guest_init", include_str!("guest_init.rs")),
            ("storage", include_str!("storage.rs")),
        ] {
            let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
            assert!(
                !production_source.to_lowercase().contains("buildah"),
                "{name} production code should not know buildah"
            );
        }
        assert!(include_str!("image_cache.rs").contains("Buildah"));
    }
}
