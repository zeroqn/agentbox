//! Direct libkrun launch sequencing from a prepared `LaunchConfig`.

use anyhow::{Context, Result, anyhow, bail};
use std::path::Path;

use crate::runtime::launch::config::{LaunchConfig, NetworkMode};

use super::api::LibkrunApi;

#[derive(Debug)]
pub(crate) struct DirectLibkrunLauncher<A> {
    api: A,
}

impl<A: LibkrunApi> DirectLibkrunLauncher<A> {
    pub(crate) fn new(api: A) -> Self {
        Self { api }
    }

    #[cfg(test)]
    pub(crate) fn start_enter(self, config: &LaunchConfig) -> Result<()> {
        self.start_enter_with_pre_enter_hook(config, || {})
    }

    #[cfg(test)]
    pub(crate) fn start_enter_with_pre_enter_hook(
        self,
        config: &LaunchConfig,
        before_start_enter: impl FnOnce(),
    ) -> Result<()> {
        self.start_enter_profiled_with_pre_enter_hook(config, None, before_start_enter)
    }

    pub(crate) fn start_enter_profiled_with_pre_enter_hook(
        mut self,
        config: &LaunchConfig,
        profile_path: Option<&Path>,
        before_start_enter: impl FnOnce(),
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
        if let Err(err) = self.configure_and_start(ctx_id, config, profile_path, before_start_enter)
        {
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
        before_start_enter: impl FnOnce(),
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
        if config.network_mode == NetworkMode::Passt {
            let passt_socket = config.passt_socket.as_deref().ok_or_else(|| {
                anyhow!("libkrun passt setup requires a prepared passt unix socket path")
            })?;
            tracing::debug!(ctx_id, socket = %passt_socket.display(), "krun_add_net_unixstream: begin");
            let rc = self.api.add_net_unixstream(ctx_id, passt_socket)?;
            check_setup("krun_add_net_unixstream", rc)?;
            tracing::debug!(ctx_id, "krun_add_net_unixstream: complete");
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
        }
        before_start_enter();
        tracing::debug!(ctx_id, "krun_start_enter: begin");
        let rc = self.api.start_enter(ctx_id)?;
        tracing::debug!(ctx_id, rc, "krun_start_enter: returned");
        check_start("krun_start_enter", rc)
    }
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
