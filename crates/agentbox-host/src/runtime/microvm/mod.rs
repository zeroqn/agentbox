pub(crate) mod ffi;
pub(crate) mod guest_init;
pub(crate) mod image_cache;
pub(crate) mod launch;
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
use self::storage::{HostStorageProbe, StorageManager, StorageProbe};
use self::supervisor::{HostMicrovmSupervisor, MicrovmSupervisor};

struct MicrovmRunRequest<'a> {
    common: CommonOptions,
    options: MicrovmOptions,
    state_layout: &'a crate::state::StateLayout,
    task_id: &'a str,
    workspace_source: &'a Path,
}

struct MicrovmRunDeps<'a, B, S, M> {
    buildah: &'a B,
    storage_probe: &'a S,
    supervisor: &'a M,
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
        },
    )
}

fn run_with_deps(
    request: MicrovmRunRequest<'_>,
    deps: MicrovmRunDeps<'_, impl BuildahRunner, impl StorageProbe, impl MicrovmSupervisor>,
) -> Result<ExitCode> {
    if request.common.pull_latest {
        anyhow::bail!(pull_latest_not_supported_message());
    }

    let reference = ImageReference::from_cli(request.common.image.as_deref());
    let cache = ImageCache::new(request.state_layout.microvm_image_cache_dir());
    let entry = cache.ensure(reference, deps.buildah)?;
    let backend = StorageManager::select_backend(request.options.storage, deps.storage_probe)?;
    let storage = StorageManager::new(request.state_layout.root_dir().to_path_buf());
    let handle = storage.materialize(
        &entry,
        backend,
        request.task_id,
        request.options.preserve_debug,
    )?;
    let task_state_dir = handle
        .root
        .parent()
        .ok_or_else(|| anyhow::anyhow!("microvm task rootfs has no parent state dir"))?
        .to_path_buf();
    let guest_init = resolve_guest_init(&handle.root, request.options.guest_init.as_deref())?;
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
    })?;

    let supervisor_result = deps.supervisor.run(&config, &task_state_dir);
    let cleanup_result = handle.cleanup();
    let status = supervisor_result?;
    cleanup_result?;
    Ok(status.exit_code())
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
    fn microvm_direct_launch_modules_do_not_render_podman_or_oci_runtimes() {
        for (name, source) in [
            ("launch", include_str!("launch.rs")),
            ("supervisor", include_str!("supervisor.rs")),
            ("ffi", include_str!("ffi.rs")),
            ("guest_init", include_str!("guest_init.rs")),
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
