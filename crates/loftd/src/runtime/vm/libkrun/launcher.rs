//! Direct libkrun launch sequencing from a prepared `LaunchConfig`.

use anyhow::{Context, Result, anyhow, bail};
#[cfg(test)]
use std::cell::RefCell;
use std::path::Path;

use crate::runtime::launch::config::{LaunchConfig, NetworkMode};
use crate::runtime::publish::tsi_port_map;
use crate::runtime::seccomp::{self, SeccompMode};
use crate::runtime::session::supervisor::rlimits::host_nofile_hard_limit;
use crate::runtime::vm::gpu::GpuMode;

use crate::runtime::vm::libkrun::api::LibkrunApi;

pub(in crate::runtime::vm::libkrun) const PROFILE_KERNEL_CMDLINE_APPEND: &str =
    "ignore_loglevel loglevel=7 printk.time=1 initcall_debug";
pub(in crate::runtime::vm::libkrun) const NET_FLAG_DHCP_CLIENT: u32 = 1 << 1;
const VIRGLRENDERER_USE_EGL: u32 = 1 << 0;
const VIRGLRENDERER_THREAD_SYNC: u32 = 1 << 1;
const VIRGLRENDERER_NO_VIRGL: u32 = 1 << 7;
const VIRGLRENDERER_USE_ASYNC_FENCE_CB: u32 = 1 << 8;
const VIRGLRENDERER_DRM: u32 = 1 << 10;
const VIRGLRENDERER_NATIVE_CONTEXT_FLAGS: u32 = VIRGLRENDERER_USE_EGL
    | VIRGLRENDERER_THREAD_SYNC
    | VIRGLRENDERER_NO_VIRGL
    | VIRGLRENDERER_USE_ASYNC_FENCE_CB
    | VIRGLRENDERER_DRM;
const GPU_SHM_SIZE_BYTES: u64 = 256 * 1024 * 1024;

#[cfg(test)]
type AuditStartMarkerHook = Box<dyn FnMut() -> Result<()>>;

#[cfg(test)]
thread_local! {
    static AUDIT_START_MARKER_HOOK: RefCell<Option<AuditStartMarkerHook>> =
        RefCell::new(None);
}

#[cfg(test)]
pub(in crate::runtime) fn with_audit_start_marker_hook_for_test<T>(
    hook: impl FnMut() -> Result<()> + 'static,
    action: impl FnOnce() -> T,
) -> T {
    struct MarkerHookGuard;

    impl Drop for MarkerHookGuard {
        fn drop(&mut self) {
            AUDIT_START_MARKER_HOOK.with(|hook| {
                *hook.borrow_mut() = None;
            });
        }
    }

    AUDIT_START_MARKER_HOOK.with(|slot| {
        assert!(
            slot.borrow().is_none(),
            "nested seccomp audit marker hooks are not supported"
        );
        *slot.borrow_mut() = Some(Box::new(hook));
    });
    let _guard = MarkerHookGuard;
    action()
}

#[derive(Debug)]
pub(in crate::runtime) struct DirectLibkrunLauncher<A> {
    api: A,
}

impl<A: LibkrunApi> DirectLibkrunLauncher<A> {
    pub(in crate::runtime) fn new(api: A) -> Self {
        Self { api }
    }

    #[cfg(test)]
    pub(in crate::runtime) fn start_enter(self, config: &LaunchConfig) -> Result<()> {
        self.start_enter_with_pre_enter_hook(config, || Ok(()))
    }

    #[cfg(test)]
    pub(in crate::runtime) fn start_enter_with_pre_enter_hook(
        self,
        config: &LaunchConfig,
        before_start_enter: impl FnOnce() -> Result<()>,
    ) -> Result<()> {
        self.start_enter_profiled_with_pre_enter_hook(config, None, before_start_enter)
    }

    pub(in crate::runtime) fn start_enter_profiled_with_pre_enter_hook(
        self,
        config: &LaunchConfig,
        profile_path: Option<&Path>,
        before_start_enter: impl FnOnce() -> Result<()>,
    ) -> Result<()> {
        let host_nofile_hard_limit = host_nofile_hard_limit()?;
        self.start_enter_profiled_with_nofile_hard_limit(
            config,
            profile_path,
            before_start_enter,
            host_nofile_hard_limit,
        )
    }

    #[cfg(test)]
    pub(in crate::runtime) fn start_enter_with_host_nofile_hard_limit(
        self,
        config: &LaunchConfig,
        host_nofile_hard_limit: libc::rlim_t,
    ) -> Result<()> {
        self.start_enter_profiled_with_nofile_hard_limit(
            config,
            None,
            || Ok(()),
            host_nofile_hard_limit,
        )
    }

    fn start_enter_profiled_with_nofile_hard_limit(
        mut self,
        config: &LaunchConfig,
        profile_path: Option<&Path>,
        before_start_enter: impl FnOnce() -> Result<()>,
        host_nofile_hard_limit: libc::rlim_t,
    ) -> Result<()> {
        tracing::debug!(level = ?config.log_level, libkrun_level = config.log_level.libkrun_level(), "libkrun log init: begin");
        check_setup(
            "libkrun_log_init",
            self.api.init_log(config.log_level.libkrun_level())?,
        )?;
        tracing::debug!("libkrun log init: complete");
        tracing::debug!("krun_create_ctx: begin");
        let ctx_id = self
            .api
            .create_ctx()
            .context("libkrun setup failed: create ctx")?;
        tracing::debug!(ctx_id, "krun_create_ctx: complete");
        if let Err(err) = self.configure_and_start(
            ctx_id,
            config,
            profile_path,
            before_start_enter,
            host_nofile_hard_limit,
        ) {
            let _ = self.api.free_ctx(ctx_id);
            return Err(err);
        }
        Ok(())
    }

    fn configure_and_start(
        &mut self,
        ctx_id: u32,
        config: &LaunchConfig,
        profile_path: Option<&Path>,
        before_start_enter: impl FnOnce() -> Result<()>,
        host_nofile_hard_limit: libc::rlim_t,
    ) -> Result<()> {
        tracing::debug!(
            ctx_id,
            vcpus = config.vcpus,
            ram_mib = config.ram_mib,
            "krun_set_vm_config: begin"
        );
        let rc = self
            .api
            .set_vm_config(ctx_id, config.vcpus, config.ram_mib)?;
        check_setup("krun_set_vm_config", rc)?;
        tracing::debug!(ctx_id, "krun_set_vm_config: complete");
        self.configure_gpu(ctx_id, config.gpu_mode)?;
        self.configure_nested_virt(ctx_id)?;
        tracing::debug!(ctx_id, rootfs = %config.task_rootfs.display(), "krun_set_root: begin");
        let rc = self.api.set_root(ctx_id, &config.task_rootfs)?;
        check_setup("krun_set_root", rc)?;
        tracing::debug!(ctx_id, "krun_set_root: complete");
        for disk in &config.disks {
            tracing::debug!(ctx_id, disk_id = %disk.id, disk_path = %disk.path.display(), read_only = disk.read_only, "krun_add_disk: begin");
            let rc = self
                .api
                .add_disk(ctx_id, &disk.id, &disk.path, disk.read_only)?;
            check_setup("krun_add_disk", rc)?;
            tracing::debug!(ctx_id, disk_id = %disk.id, "krun_add_disk: complete");
        }
        if let Some(waypipe) = &config.waypipe {
            tracing::debug!(
                ctx_id,
                guest_port = waypipe.guest_port,
                socket = %waypipe.socket.display(),
                "krun_add_vsock_port2: Waypipe begin"
            );
            let rc = self
                .api
                .add_vsock_port(ctx_id, waypipe.guest_port, &waypipe.socket, false)?;
            check_setup("krun_add_vsock_port2 Waypipe", rc)?;
            tracing::debug!(ctx_id, "krun_add_vsock_port2: Waypipe mapping registered");
        }
        if let Some(managed) = &config.managed_session {
            tracing::debug!(
                ctx_id,
                guest_port = managed.guest_port,
                socket = %managed.attach_socket.display(),
                "krun_add_vsock_port2: begin"
            );
            let rc = self.api.add_vsock_port(
                ctx_id,
                managed.guest_port,
                &managed.attach_socket,
                true,
            )?;
            check_setup("krun_add_vsock_port2", rc)?;
            tracing::debug!(ctx_id, "krun_add_vsock_port2: mapping registered");
        }
        if config.network_mode == NetworkMode::Passt {
            let passt_fd = config.passt_fd.ok_or_else(|| {
                anyhow!("libkrun passt setup requires a prepared passt socket fd")
            })?;
            tracing::debug!(
                ctx_id,
                fd = passt_fd,
                flags = NET_FLAG_DHCP_CLIENT,
                "krun_add_net_unixstream: begin"
            );
            let mut rc = self
                .api
                .add_net_unixstream(ctx_id, passt_fd, NET_FLAG_DHCP_CLIENT)?;
            if rc == -libc::EINVAL {
                tracing::debug!(
                    ctx_id,
                    fd = passt_fd,
                    "krun_add_net_unixstream returned EINVAL with NET_FLAG_DHCP_CLIENT; retrying without flags"
                );
                rc = self.api.add_net_unixstream(ctx_id, passt_fd, 0)?;
            }
            check_setup("krun_add_net_unixstream", rc)?;
            tracing::debug!(ctx_id, "krun_add_net_unixstream: complete");
        } else if !config.publish.is_empty() {
            let port_map =
                tsi_port_map(&config.publish).context("libkrun TSI publish setup failed")?;
            tracing::debug!(ctx_id, ports = ?port_map, "krun_set_port_map: begin");
            let rc = self.api.set_port_map(ctx_id, &port_map)?;
            check_setup("krun_set_port_map", rc)?;
            tracing::debug!(ctx_id, "krun_set_port_map: complete");
        }
        tracing::debug!(ctx_id, "krun_disable_implicit_console: begin");
        let rc = self.api.disable_implicit_console(ctx_id)?;
        check_setup("krun_disable_implicit_console", rc)?;
        tracing::debug!(ctx_id, "krun_disable_implicit_console: complete");
        tracing::debug!(ctx_id, "krun_add_virtio_console_default: begin");
        let rc = self.api.add_virtio_console_default(ctx_id, 0, 1, 2)?;
        check_setup("krun_add_virtio_console_default", rc)?;
        tracing::debug!(ctx_id, "krun_add_virtio_console_default: complete");
        tracing::debug!(
            ctx_id,
            prepared_root_bind_count = config.mounts.len(),
            "krun_add_virtiofs3: skipped for prepared-root developer binds"
        );
        tracing::debug!(ctx_id, workdir = %config.workdir, "krun_set_workdir: begin");
        let rc = self.api.set_workdir(ctx_id, &config.workdir)?;
        check_setup("krun_set_workdir", rc)?;
        tracing::debug!(ctx_id, "krun_set_workdir: complete");
        tracing::debug!(ctx_id, exec_path = %config.exec_path, argv_len = config.argv.len(), env_len = config.env.len(), "krun_set_exec: begin");
        let rc = self
            .api
            .set_exec(ctx_id, &config.exec_path, &config.argv, &config.env)?;
        check_setup("krun_set_exec", rc)?;
        tracing::debug!(ctx_id, "krun_set_exec: complete");
        if let Some(profile_path) = profile_path {
            tracing::debug!(ctx_id, profile_path = %profile_path.display(), "krun_set_profile_path: begin");
            let rc = self.api.set_profile_path(ctx_id, profile_path)?;
            check_setup("krun_set_profile_path", rc)?;
            tracing::debug!(ctx_id, "krun_set_profile_path: complete");
            tracing::debug!(
                ctx_id,
                fragment = PROFILE_KERNEL_CMDLINE_APPEND,
                "krun_set_kernel_cmdline_append: begin"
            );
            let rc = self
                .api
                .set_kernel_cmdline_append(ctx_id, PROFILE_KERNEL_CMDLINE_APPEND)?;
            check_setup("krun_set_kernel_cmdline_append", rc)?;
            tracing::debug!(ctx_id, "krun_set_kernel_cmdline_append: complete");
        }
        let guest_nofile_rlimit = guest_nofile_rlimit_entry(host_nofile_hard_limit);
        tracing::debug!(
            ctx_id,
            host_nofile_hard_limit,
            rlimit = %guest_nofile_rlimit,
            "krun_set_rlimits: begin"
        );
        let rc = self.api.set_rlimits(ctx_id, &[guest_nofile_rlimit])?;
        check_setup("krun_set_rlimits", rc)?;
        tracing::debug!(ctx_id, "krun_set_rlimits: complete");
        before_start_enter()?;
        tracing::debug!(ctx_id, "krun_start_enter: begin");
        emit_audit_start_marker_for_launch(config)?;
        let rc = self.api.start_enter(ctx_id)?;
        tracing::debug!(
            ctx_id,
            rc,
            "krun_start_enter returned before successful VM takeover"
        );
        check_start("krun_start_enter", rc)
    }

    fn configure_gpu(&mut self, ctx_id: u32, gpu_mode: GpuMode) -> Result<()> {
        match gpu_mode {
            GpuMode::Off => Ok(()),
            GpuMode::Drm => {
                tracing::debug!(
                    ctx_id,
                    virgl_flags = VIRGLRENDERER_NATIVE_CONTEXT_FLAGS,
                    shm_size = GPU_SHM_SIZE_BYTES,
                    "krun_set_gpu_options2: begin"
                );
                let rc = self
                    .api
                    .set_gpu_options2(
                        ctx_id,
                        VIRGLRENDERER_NATIVE_CONTEXT_FLAGS,
                        GPU_SHM_SIZE_BYTES,
                    )?
                    .ok_or_else(|| {
                        anyhow!("libkrun setup failed: krun_set_gpu_options2 is unavailable")
                    })?;
                check_setup("krun_set_gpu_options2", rc)?;
                tracing::debug!(ctx_id, "krun_set_gpu_options2: complete");
                Ok(())
            }
        }
    }

    fn configure_nested_virt(&mut self, ctx_id: u32) -> Result<()> {
        tracing::debug!(ctx_id, "krun_check_nested_virt: begin");
        match self.api.check_nested_virt()? {
            Some(1) => {
                tracing::debug!(
                    ctx_id,
                    "krun_check_nested_virt: host nested virtualization supported"
                );
            }
            Some(0) => {
                tracing::warn!(
                    ctx_id,
                    "krun_check_nested_virt: host nested virtualization is not reported as supported; requesting nested virtualization anyway"
                );
            }
            Some(rc) => {
                tracing::warn!(
                    ctx_id,
                    rc,
                    "krun_check_nested_virt: support check failed; requesting nested virtualization anyway"
                );
            }
            None => {
                tracing::debug!(
                    ctx_id,
                    "krun_check_nested_virt: optional diagnostic symbol unavailable; requesting nested virtualization anyway"
                );
            }
        }
        tracing::debug!(ctx_id, "krun_set_nested_virt: begin");
        let rc = self.api.set_nested_virt(ctx_id, true)?;
        check_setup("krun_set_nested_virt", rc)?;
        tracing::debug!(ctx_id, "krun_set_nested_virt: complete");
        Ok(())
    }
}

fn emit_audit_start_marker_for_launch(config: &LaunchConfig) -> Result<()> {
    if !matches!(config.seccomp, SeccompMode::Audit(_)) {
        return Ok(());
    }

    #[cfg(test)]
    {
        let hook_result = AUDIT_START_MARKER_HOOK.with(|slot| {
            let mut hook = slot.borrow_mut();
            hook.as_mut().map(|hook| hook())
        });
        if let Some(result) = hook_result {
            return result;
        }
    }

    seccomp::emit_audit_start_marker()
}

pub(in crate::runtime::vm::libkrun) fn guest_nofile_rlimit_entry(
    host_nofile_hard_limit: libc::rlim_t,
) -> String {
    // libkrun expects Linux resource numeric IDs in RESOURCE=RLIM_CUR:RLIM_MAX form.
    format!(
        "{}={}:{}",
        libc::RLIMIT_NOFILE,
        host_nofile_hard_limit,
        host_nofile_hard_limit
    )
}

fn check_setup(name: &str, rc: i32) -> Result<()> {
    if rc < 0 {
        bail!("libkrun setup failed: {name} returned {rc}");
    }
    Ok(())
}

fn check_start(name: &str, rc: i32) -> Result<()> {
    if rc < 0 {
        bail!("libkrun start failed: {name} returned {rc}");
    }
    Ok(())
}
