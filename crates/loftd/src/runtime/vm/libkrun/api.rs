//! libkrun API surface used by the direct launcher.

use anyhow::Result;
use std::path::Path;

pub(crate) trait LibkrunApi {
    fn create_ctx(&mut self) -> Result<u32>;
    fn free_ctx(&mut self, ctx_id: u32) -> Result<()>;
    fn init_log(&mut self, level: u32) -> Result<i32>;
    fn set_vm_config(&mut self, ctx_id: u32, vcpus: u8, ram_mib: u32) -> Result<i32>;
    fn check_nested_virt(&mut self) -> Result<Option<i32>>;
    fn set_nested_virt(&mut self, ctx_id: u32, enabled: bool) -> Result<i32>;
    fn set_root(&mut self, ctx_id: u32, root_path: &Path) -> Result<i32>;
    fn add_disk(
        &mut self,
        ctx_id: u32,
        block_id: &str,
        disk_path: &Path,
        read_only: bool,
    ) -> Result<i32>;
    fn disable_implicit_console(&mut self, ctx_id: u32) -> Result<i32>;
    fn add_virtio_console_default(
        &mut self,
        ctx_id: u32,
        input_fd: i32,
        output_fd: i32,
        err_fd: i32,
    ) -> Result<i32>;
    fn add_net_unixstream(&mut self, ctx_id: u32, socket_fd: i32, flags: u32) -> Result<i32>;
    fn set_port_map(&mut self, ctx_id: u32, port_map: &[String]) -> Result<i32>;
    fn set_workdir(&mut self, ctx_id: u32, workdir: &str) -> Result<i32>;
    fn set_exec(
        &mut self,
        ctx_id: u32,
        exec_path: &str,
        argv: &[String],
        env: &[(String, String)],
    ) -> Result<i32>;
    fn set_profile_path(&mut self, ctx_id: u32, profile_path: &Path) -> Result<i32>;
    fn set_kernel_cmdline_append(&mut self, ctx_id: u32, fragment: &str) -> Result<i32>;
    fn start_enter(&mut self, ctx_id: u32) -> Result<i32>;
}
