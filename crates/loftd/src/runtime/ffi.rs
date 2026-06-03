use anyhow::{Context, Result, anyhow, bail};
use std::ffi::CString;
use std::os::raw::{c_char, c_void};
use std::path::Path;

use crate::runtime::launch_config::LaunchConfig;

const DAX_SHM_SIZE: u64 = 0;
const READ_WRITE: bool = false;
const LIBKRUN_LIBRARY_ENV: &str = "LOFTD_LIBKRUN_LIBRARY";
const DEFAULT_LIBKRUN_NAMES: [&str; 2] = ["libkrun.so.1", "libkrun.so"];

pub(crate) trait LibkrunApi {
    fn create_ctx(&mut self) -> Result<u32>;
    fn free_ctx(&mut self, ctx_id: u32) -> Result<()>;
    fn set_log_level(&mut self, level: u32) -> Result<i32>;
    fn set_vm_config(&mut self, ctx_id: u32, vcpus: u8, ram_mib: u32) -> Result<i32>;
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
    fn add_virtiofs3(
        &mut self,
        ctx_id: u32,
        tag: &str,
        host_path: &Path,
        shm_size: u64,
        read_only: bool,
    ) -> Result<i32>;
    fn set_workdir(&mut self, ctx_id: u32, workdir: &str) -> Result<i32>;
    fn set_exec(
        &mut self,
        ctx_id: u32,
        exec_path: &str,
        argv: &[String],
        env: &[(String, String)],
    ) -> Result<i32>;
    fn start_enter(&mut self, ctx_id: u32) -> Result<i32>;
}

#[derive(Debug)]
pub(crate) struct DirectLibkrunLauncher<A> {
    api: A,
}

impl<A: LibkrunApi> DirectLibkrunLauncher<A> {
    pub(crate) fn new(api: A) -> Self {
        Self { api }
    }

    pub(crate) fn start_enter(mut self, config: &LaunchConfig) -> Result<()> {
        let ctx_id = self
            .api
            .create_ctx()
            .context("libkrun setup failed: create ctx")?;
        if let Err(err) = self.configure_and_start(ctx_id, config) {
            let _ = self.api.free_ctx(ctx_id);
            return Err(err);
        }
        Ok(())
    }

    fn configure_and_start(&mut self, ctx_id: u32, config: &LaunchConfig) -> Result<()> {
        check_setup("krun_set_log_level", self.api.set_log_level(0)?)?;
        let rc = self
            .api
            .set_vm_config(ctx_id, config.vcpus, config.ram_mib)?;
        check_setup("krun_set_vm_config", rc)?;
        let rc = self.api.set_root(ctx_id, &config.task_rootfs)?;
        check_setup("krun_set_root", rc)?;
        for disk in &config.disks {
            let rc = self
                .api
                .add_disk(ctx_id, &disk.id, &disk.path, disk.read_only)?;
            check_setup("krun_add_disk", rc)?;
        }
        let rc = self.api.disable_implicit_console(ctx_id)?;
        check_setup("krun_disable_implicit_console", rc)?;
        let rc = self.api.add_virtio_console_default(ctx_id, 0, 1, 2)?;
        check_setup("krun_add_virtio_console_default", rc)?;
        let rc = self.api.add_virtiofs3(
            ctx_id,
            &config.workspace.tag,
            &config.workspace.source,
            DAX_SHM_SIZE,
            READ_WRITE,
        )?;
        check_setup("krun_add_virtiofs3", rc)?;
        let rc = self.api.set_workdir(ctx_id, &config.workdir)?;
        check_setup("krun_set_workdir", rc)?;
        let rc = self
            .api
            .set_exec(ctx_id, &config.exec_path, &config.argv, &config.env)?;
        check_setup("krun_set_exec", rc)?;
        let rc = self.api.start_enter(ctx_id)?;
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

type KrunSetLogLevel = unsafe extern "C" fn(u32) -> i32;
type KrunCreateCtx = unsafe extern "C" fn() -> i32;
type KrunFreeCtx = unsafe extern "C" fn(u32) -> i32;
type KrunSetVmConfig = unsafe extern "C" fn(u32, u8, u32) -> i32;
type KrunSetRoot = unsafe extern "C" fn(u32, *const c_char) -> i32;
type KrunAddDisk = unsafe extern "C" fn(u32, *const c_char, *const c_char, bool) -> i32;
type KrunDisableImplicitConsole = unsafe extern "C" fn(u32) -> i32;
type KrunAddVirtioConsoleDefault = unsafe extern "C" fn(u32, i32, i32, i32) -> i32;
type KrunAddVirtiofs3 = unsafe extern "C" fn(u32, *const c_char, *const c_char, u64, bool) -> i32;
type KrunSetWorkdir = unsafe extern "C" fn(u32, *const c_char) -> i32;
type KrunSetExec =
    unsafe extern "C" fn(u32, *const c_char, *const *const c_char, *const *const c_char) -> i32;
type KrunStartEnter = unsafe extern "C" fn(u32) -> i32;

pub(crate) struct DynamicLibkrunApi {
    handle: *mut c_void,
    set_log_level: KrunSetLogLevel,
    create_ctx: KrunCreateCtx,
    free_ctx: KrunFreeCtx,
    set_vm_config: KrunSetVmConfig,
    set_root: KrunSetRoot,
    add_disk: KrunAddDisk,
    disable_implicit_console: KrunDisableImplicitConsole,
    add_virtio_console_default: KrunAddVirtioConsoleDefault,
    add_virtiofs3: KrunAddVirtiofs3,
    set_workdir: KrunSetWorkdir,
    set_exec: KrunSetExec,
    start_enter: KrunStartEnter,
}

impl DynamicLibkrunApi {
    pub(crate) fn open_default() -> Result<Self> {
        if let Some(path) = explicit_libkrun_library_override()? {
            return Self::open(&path).with_context(|| {
                format!(
                    "{LIBKRUN_LIBRARY_ENV} points to '{}', but that libkrun library could not be loaded",
                    path
                )
            });
        }

        let mut last_error = None;
        for name in DEFAULT_LIBKRUN_NAMES {
            match Self::open(name) {
                Ok(api) => return Ok(api),
                Err(err) => last_error = Some(err),
            }
        }
        Err(last_error.unwrap_or_else(|| anyhow!("failed to load libkrun")))
    }

    fn open(name: &str) -> Result<Self> {
        let c_name = CString::new(name)?;
        // SAFETY: dlopen is called with a NUL-terminated library name and RTLD_NOW.
        let handle = unsafe { libc::dlopen(c_name.as_ptr(), libc::RTLD_NOW) };
        if handle.is_null() {
            bail!("failed to load {name}: {}", dlerror_string());
        }

        let api =
            unsafe {
                // SAFETY: symbols are resolved from a successfully opened libkrun handle and
                // transmuted to signatures verified against the local libkrun 1.18 header.
                Self {
                    handle,
                    set_log_level: std::mem::transmute::<*mut c_void, KrunSetLogLevel>(
                        load_symbol(handle, "krun_set_log_level")?,
                    ),
                    create_ctx: std::mem::transmute::<*mut c_void, KrunCreateCtx>(load_symbol(
                        handle,
                        "krun_create_ctx",
                    )?),
                    free_ctx: std::mem::transmute::<*mut c_void, KrunFreeCtx>(load_symbol(
                        handle,
                        "krun_free_ctx",
                    )?),
                    set_vm_config: std::mem::transmute::<*mut c_void, KrunSetVmConfig>(
                        load_symbol(handle, "krun_set_vm_config")?,
                    ),
                    set_root: std::mem::transmute::<*mut c_void, KrunSetRoot>(load_symbol(
                        handle,
                        "krun_set_root",
                    )?),
                    add_disk: std::mem::transmute::<*mut c_void, KrunAddDisk>(load_symbol(
                        handle,
                        "krun_add_disk",
                    )?),
                    disable_implicit_console: std::mem::transmute::<
                        *mut c_void,
                        KrunDisableImplicitConsole,
                    >(load_symbol(
                        handle,
                        "krun_disable_implicit_console",
                    )?),
                    add_virtio_console_default: std::mem::transmute::<
                        *mut c_void,
                        KrunAddVirtioConsoleDefault,
                    >(load_symbol(
                        handle,
                        "krun_add_virtio_console_default",
                    )?),
                    add_virtiofs3: std::mem::transmute::<*mut c_void, KrunAddVirtiofs3>(
                        load_symbol(handle, "krun_add_virtiofs3")?,
                    ),
                    set_workdir: std::mem::transmute::<*mut c_void, KrunSetWorkdir>(load_symbol(
                        handle,
                        "krun_set_workdir",
                    )?),
                    set_exec: std::mem::transmute::<*mut c_void, KrunSetExec>(load_symbol(
                        handle,
                        "krun_set_exec",
                    )?),
                    start_enter: std::mem::transmute::<*mut c_void, KrunStartEnter>(load_symbol(
                        handle,
                        "krun_start_enter",
                    )?),
                }
            };
        Ok(api)
    }
}

fn explicit_libkrun_library_override() -> Result<Option<String>> {
    match std::env::var(LIBKRUN_LIBRARY_ENV) {
        Ok(value) if value.trim().is_empty() => Ok(None),
        Ok(value) => Ok(Some(value)),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(std::env::VarError::NotUnicode(_)) => {
            bail!("{LIBKRUN_LIBRARY_ENV} must be valid UTF-8")
        }
    }
}

#[cfg(test)]
fn planned_libkrun_load_order(override_value: Option<&str>) -> Vec<String> {
    if let Some(value) = override_value
        && !value.trim().is_empty()
    {
        return vec![value.to_owned()];
    }
    DEFAULT_LIBKRUN_NAMES
        .iter()
        .map(|name| (*name).to_owned())
        .collect()
}

impl Drop for DynamicLibkrunApi {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            // SAFETY: handle was returned by dlopen and is owned by this struct.
            unsafe { libc::dlclose(self.handle) };
        }
    }
}

impl LibkrunApi for DynamicLibkrunApi {
    fn create_ctx(&mut self) -> Result<u32> {
        // SAFETY: function pointer resolved from libkrun with verified signature.
        let rc = unsafe { (self.create_ctx)() };
        if rc < 0 {
            bail!("krun_create_ctx returned {rc}");
        }
        u32::try_from(rc).context("krun_create_ctx returned invalid context id")
    }

    fn free_ctx(&mut self, ctx_id: u32) -> Result<()> {
        // SAFETY: function pointer resolved from libkrun with verified signature.
        let rc = unsafe { (self.free_ctx)(ctx_id) };
        if rc < 0 {
            bail!("krun_free_ctx returned {rc}");
        }
        Ok(())
    }

    fn set_log_level(&mut self, level: u32) -> Result<i32> {
        // SAFETY: function pointer resolved from libkrun with verified signature.
        Ok(unsafe { (self.set_log_level)(level) })
    }

    fn set_vm_config(&mut self, ctx_id: u32, vcpus: u8, ram_mib: u32) -> Result<i32> {
        // SAFETY: function pointer resolved from libkrun with verified signature.
        Ok(unsafe { (self.set_vm_config)(ctx_id, vcpus, ram_mib) })
    }

    fn set_root(&mut self, ctx_id: u32, root_path: &Path) -> Result<i32> {
        let root_path = path_cstring(root_path)?;
        // SAFETY: C string lives for the duration of the call.
        Ok(unsafe { (self.set_root)(ctx_id, root_path.as_ptr()) })
    }

    fn add_disk(
        &mut self,
        ctx_id: u32,
        block_id: &str,
        disk_path: &Path,
        read_only: bool,
    ) -> Result<i32> {
        let block_id = CString::new(block_id)?;
        let disk_path = path_cstring(disk_path)?;
        // SAFETY: C strings live for the duration of the call.
        Ok(unsafe { (self.add_disk)(ctx_id, block_id.as_ptr(), disk_path.as_ptr(), read_only) })
    }

    fn disable_implicit_console(&mut self, ctx_id: u32) -> Result<i32> {
        // SAFETY: function pointer resolved from libkrun with verified signature.
        Ok(unsafe { (self.disable_implicit_console)(ctx_id) })
    }

    fn add_virtio_console_default(
        &mut self,
        ctx_id: u32,
        input_fd: i32,
        output_fd: i32,
        err_fd: i32,
    ) -> Result<i32> {
        // SAFETY: function pointer resolved from libkrun with verified signature.
        Ok(unsafe { (self.add_virtio_console_default)(ctx_id, input_fd, output_fd, err_fd) })
    }

    fn add_virtiofs3(
        &mut self,
        ctx_id: u32,
        tag: &str,
        host_path: &Path,
        shm_size: u64,
        read_only: bool,
    ) -> Result<i32> {
        let tag = CString::new(tag)?;
        let host_path = path_cstring(host_path)?;
        // SAFETY: C strings live for the duration of the call.
        Ok(unsafe {
            (self.add_virtiofs3)(
                ctx_id,
                tag.as_ptr(),
                host_path.as_ptr(),
                shm_size,
                read_only,
            )
        })
    }

    fn set_workdir(&mut self, ctx_id: u32, workdir: &str) -> Result<i32> {
        let workdir = CString::new(workdir)?;
        // SAFETY: C string lives for the duration of the call.
        Ok(unsafe { (self.set_workdir)(ctx_id, workdir.as_ptr()) })
    }

    fn set_exec(
        &mut self,
        ctx_id: u32,
        exec_path: &str,
        argv: &[String],
        env: &[(String, String)],
    ) -> Result<i32> {
        let exec_path = CString::new(exec_path)?;
        let argv_strings = argv
            .iter()
            .map(|arg| CString::new(arg.as_str()))
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let env_strings = env
            .iter()
            .map(|(key, value)| CString::new(format!("{key}={value}")))
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let argv = null_terminated_ptrs(&argv_strings);
        let envp = null_terminated_ptrs(&env_strings);
        // SAFETY: C strings and pointer arrays live for the duration of the call.
        Ok(unsafe { (self.set_exec)(ctx_id, exec_path.as_ptr(), argv.as_ptr(), envp.as_ptr()) })
    }

    fn start_enter(&mut self, ctx_id: u32) -> Result<i32> {
        // SAFETY: function pointer resolved from libkrun with verified signature. libkrun may
        // exit the current helper process after this call; parent cleanup runs outside it.
        Ok(unsafe { (self.start_enter)(ctx_id) })
    }
}

fn path_cstring(path: &Path) -> Result<CString> {
    use std::os::unix::ffi::OsStrExt;
    CString::new(path.as_os_str().as_bytes()).context("path contains interior NUL")
}

fn null_terminated_ptrs(values: &[CString]) -> Vec<*const c_char> {
    let mut ptrs = values
        .iter()
        .map(|value| value.as_ptr())
        .collect::<Vec<_>>();
    ptrs.push(std::ptr::null());
    ptrs
}

unsafe fn load_symbol(handle: *mut c_void, name: &str) -> Result<*mut c_void> {
    let c_name = CString::new(name)?;
    // SAFETY: handle is an open dlopen handle and c_name is NUL-terminated.
    let symbol = unsafe { libc::dlsym(handle, c_name.as_ptr()) };
    if symbol.is_null() {
        bail!(
            "failed to resolve libkrun symbol {name}: {}",
            dlerror_string()
        );
    }
    Ok(symbol)
}

fn dlerror_string() -> String {
    // SAFETY: dlerror returns a thread-local NUL-terminated error string or NULL.
    let err = unsafe { libc::dlerror() };
    if err.is_null() {
        return "unknown dlerror".to_owned();
    }
    // SAFETY: non-null dlerror pointer is valid until the next dl* call.
    unsafe { std::ffi::CStr::from_ptr(err) }
        .to_string_lossy()
        .into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::launch_config::{DiskAttachment, LaunchSpec};
    use std::cell::RefCell;
    use std::path::Path;
    use std::rc::Rc;

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum Call {
        CreateCtx,
        FreeCtx(u32),
        SetLogLevel(u32),
        SetVmConfig(u32, u8, u32),
        SetRoot(u32, String),
        AddDisk(u32, String, String, bool),
        DisableImplicitConsole(u32),
        AddVirtioConsoleDefault(u32, i32, i32, i32),
        AddVirtiofs3(u32, String, String, u64, bool),
        SetWorkdir(u32, String),
        SetExec(u32, String, Vec<String>, Vec<(String, String)>),
        StartEnter(u32),
    }

    #[derive(Clone)]
    struct FakeLibkrunApi {
        calls: Rc<RefCell<Vec<Call>>>,
        fail_call: Option<&'static str>,
    }

    impl FakeLibkrunApi {
        fn new(calls: Rc<RefCell<Vec<Call>>>) -> Self {
            Self {
                calls,
                fail_call: None,
            }
        }

        fn failing(calls: Rc<RefCell<Vec<Call>>>, fail_call: &'static str) -> Self {
            Self {
                calls,
                fail_call: Some(fail_call),
            }
        }

        fn rc(&self, call: &'static str) -> i32 {
            if self.fail_call == Some(call) { -22 } else { 0 }
        }
    }

    impl LibkrunApi for FakeLibkrunApi {
        fn create_ctx(&mut self) -> Result<u32> {
            self.calls.borrow_mut().push(Call::CreateCtx);
            Ok(7)
        }

        fn free_ctx(&mut self, ctx_id: u32) -> Result<()> {
            self.calls.borrow_mut().push(Call::FreeCtx(ctx_id));
            Ok(())
        }

        fn set_log_level(&mut self, level: u32) -> Result<i32> {
            self.calls.borrow_mut().push(Call::SetLogLevel(level));
            Ok(self.rc("krun_set_log_level"))
        }

        fn set_vm_config(&mut self, ctx_id: u32, vcpus: u8, ram_mib: u32) -> Result<i32> {
            self.calls
                .borrow_mut()
                .push(Call::SetVmConfig(ctx_id, vcpus, ram_mib));
            Ok(self.rc("krun_set_vm_config"))
        }

        fn set_root(&mut self, ctx_id: u32, root_path: &Path) -> Result<i32> {
            self.calls
                .borrow_mut()
                .push(Call::SetRoot(ctx_id, root_path.display().to_string()));
            Ok(self.rc("krun_set_root"))
        }

        fn add_disk(
            &mut self,
            ctx_id: u32,
            block_id: &str,
            disk_path: &Path,
            read_only: bool,
        ) -> Result<i32> {
            self.calls.borrow_mut().push(Call::AddDisk(
                ctx_id,
                block_id.to_owned(),
                disk_path.display().to_string(),
                read_only,
            ));
            Ok(self.rc("krun_add_disk"))
        }

        fn disable_implicit_console(&mut self, ctx_id: u32) -> Result<i32> {
            self.calls
                .borrow_mut()
                .push(Call::DisableImplicitConsole(ctx_id));
            Ok(self.rc("krun_disable_implicit_console"))
        }

        fn add_virtio_console_default(
            &mut self,
            ctx_id: u32,
            input_fd: i32,
            output_fd: i32,
            err_fd: i32,
        ) -> Result<i32> {
            self.calls.borrow_mut().push(Call::AddVirtioConsoleDefault(
                ctx_id, input_fd, output_fd, err_fd,
            ));
            Ok(self.rc("krun_add_virtio_console_default"))
        }

        fn add_virtiofs3(
            &mut self,
            ctx_id: u32,
            tag: &str,
            host_path: &Path,
            shm_size: u64,
            read_only: bool,
        ) -> Result<i32> {
            self.calls.borrow_mut().push(Call::AddVirtiofs3(
                ctx_id,
                tag.to_owned(),
                host_path.display().to_string(),
                shm_size,
                read_only,
            ));
            Ok(self.rc("krun_add_virtiofs3"))
        }

        fn set_workdir(&mut self, ctx_id: u32, workdir: &str) -> Result<i32> {
            self.calls
                .borrow_mut()
                .push(Call::SetWorkdir(ctx_id, workdir.to_owned()));
            Ok(self.rc("krun_set_workdir"))
        }

        fn set_exec(
            &mut self,
            ctx_id: u32,
            exec_path: &str,
            argv: &[String],
            env: &[(String, String)],
        ) -> Result<i32> {
            self.calls.borrow_mut().push(Call::SetExec(
                ctx_id,
                exec_path.to_owned(),
                argv.to_vec(),
                env.to_vec(),
            ));
            Ok(self.rc("krun_set_exec"))
        }

        fn start_enter(&mut self, ctx_id: u32) -> Result<i32> {
            self.calls.borrow_mut().push(Call::StartEnter(ctx_id));
            Ok(self.rc("krun_start_enter"))
        }
    }

    fn config() -> LaunchConfig {
        LaunchConfig::build_for_task(LaunchSpec {
            task_rootfs: Path::new("/rootfs"),
            workspace_source: Path::new("/workspace-src"),
            guest_init_exec: "/nix/store/hash-loftd/bin/loftd-guest-init",
            guest_command: &[],
            image_process_config: &crate::runtime::image_source::OciProcessConfig::default(),
            mem_gib: Some(4),
            debug: false,
            profile: false,
            root: false,
            host_uid: 1000,
            host_gid: 1001,
            vcpus: 2,
            disks: vec![
                DiskAttachment {
                    id: "loftd-nix".to_owned(),
                    path: Path::new("/state/loftd-nix.raw").to_path_buf(),
                    read_only: false,
                },
                DiskAttachment {
                    id: "loftd-containers".to_owned(),
                    path: Path::new("/state/loftd-containers.raw").to_path_buf(),
                    read_only: false,
                },
            ],
            extra_env: Vec::new(),
        })
        .expect("config should build")
    }

    #[test]
    fn libkrun_loader_prefers_explicit_library_override_before_sonames() {
        assert_eq!(
            planned_libkrun_load_order(Some("/tmp/libkrun-custom.so")),
            vec!["/tmp/libkrun-custom.so"]
        );
        assert_eq!(
            planned_libkrun_load_order(None),
            vec!["libkrun.so.1", "libkrun.so"]
        );
        assert_eq!(
            planned_libkrun_load_order(Some("")),
            vec!["libkrun.so.1", "libkrun.so"]
        );
    }

    #[test]
    fn fake_api_records_direct_libkrun_v1_call_order() {
        let calls = Rc::new(RefCell::new(Vec::new()));
        DirectLibkrunLauncher::new(FakeLibkrunApi::new(calls.clone()))
            .start_enter(&config())
            .expect("launch should succeed");

        let calls = calls.borrow();
        assert_eq!(calls[0], Call::CreateCtx);
        assert_eq!(calls[1], Call::SetLogLevel(0));
        assert_eq!(calls[2], Call::SetVmConfig(7, 2, 4096));
        assert_eq!(calls[3], Call::SetRoot(7, "/rootfs".to_owned()));
        assert_eq!(
            calls[4],
            Call::AddDisk(
                7,
                "loftd-nix".to_owned(),
                "/state/loftd-nix.raw".to_owned(),
                false,
            )
        );
        assert_eq!(
            calls[5],
            Call::AddDisk(
                7,
                "loftd-containers".to_owned(),
                "/state/loftd-containers.raw".to_owned(),
                false,
            )
        );
        assert_eq!(calls[6], Call::DisableImplicitConsole(7));
        assert_eq!(calls[7], Call::AddVirtioConsoleDefault(7, 0, 1, 2));
        assert_eq!(
            calls[8],
            Call::AddVirtiofs3(
                7,
                "loftd-workspace".to_owned(),
                "/workspace-src".to_owned(),
                0,
                false,
            )
        );
        assert_eq!(calls[9], Call::SetWorkdir(7, "/workspace".to_owned()));
        assert!(matches!(calls[10], Call::SetExec(7, _, _, _)));
        assert_eq!(calls[11], Call::StartEnter(7));
        assert_eq!(
            calls.len(),
            12,
            "v1 must not call krun_set_env or passt APIs"
        );
    }

    #[test]
    fn setup_failure_is_classified_and_frees_context_before_start() {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let err =
            DirectLibkrunLauncher::new(FakeLibkrunApi::failing(calls.clone(), "krun_set_root"))
                .start_enter(&config())
                .expect_err("setup failure should fail");

        assert!(format!("{err:#}").contains("libkrun setup failed"));
        let calls = calls.borrow();
        assert!(calls.contains(&Call::FreeCtx(7)));
        assert!(!calls.contains(&Call::StartEnter(7)));
    }

    #[test]
    fn start_failure_is_classified() {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let err =
            DirectLibkrunLauncher::new(FakeLibkrunApi::failing(calls.clone(), "krun_start_enter"))
                .start_enter(&config())
                .expect_err("start failure should fail");

        assert!(format!("{err:#}").contains("libkrun start failed"));
        assert!(calls.borrow().contains(&Call::StartEnter(7)));
    }
}
