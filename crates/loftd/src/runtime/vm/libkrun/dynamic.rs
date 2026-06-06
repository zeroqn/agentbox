//! Dynamic libkrun loading and symbol binding.

use anyhow::{Context, Result, anyhow, bail};
use std::ffi::CString;
use std::os::raw::{c_char, c_void};
use std::path::Path;

use super::api::LibkrunApi;

const LIBKRUN_LIBRARY_ENV: &str = "LOFTD_LIBKRUN_LIBRARY";
const DEFAULT_LIBKRUN_NAMES: [&str; 2] = ["libkrun.so.1", "libkrun.so"];
const LOFTD_LIBKRUN_LOG_TARGET_STDERR_FD: i32 = 2;
const LOFTD_LIBKRUN_LOG_STYLE_NEVER: u32 = 2;
const LOFTD_LIBKRUN_LOG_OPTION_NO_ENV: u32 = 1;
const LOFTD_NET_FEATURE_CSUM: u32 = 1 << 0;
const LOFTD_NET_FEATURE_GUEST_CSUM: u32 = 1 << 1;
const LOFTD_NET_FEATURE_GUEST_TSO4: u32 = 1 << 7;
const LOFTD_NET_FEATURE_GUEST_UFO: u32 = 1 << 10;
const LOFTD_NET_FEATURE_HOST_TSO4: u32 = 1 << 11;
const LOFTD_NET_FEATURE_HOST_UFO: u32 = 1 << 14;
pub(crate) const LOFTD_LIBKRUN_COMPAT_NET_FEATURES: u32 = LOFTD_NET_FEATURE_CSUM
    | LOFTD_NET_FEATURE_GUEST_CSUM
    | LOFTD_NET_FEATURE_GUEST_TSO4
    | LOFTD_NET_FEATURE_GUEST_UFO
    | LOFTD_NET_FEATURE_HOST_TSO4
    | LOFTD_NET_FEATURE_HOST_UFO;

type KrunSetLogLevel = unsafe extern "C" fn(u32) -> i32;
type KrunInitLog = unsafe extern "C" fn(i32, u32, u32, u32) -> i32;
type KrunCreateCtx = unsafe extern "C" fn() -> i32;
type KrunFreeCtx = unsafe extern "C" fn(u32) -> i32;
type KrunSetVmConfig = unsafe extern "C" fn(u32, u8, u32) -> i32;
type KrunSetRoot = unsafe extern "C" fn(u32, *const c_char) -> i32;
type KrunAddDisk = unsafe extern "C" fn(u32, *const c_char, *const c_char, bool) -> i32;
type KrunDisableImplicitConsole = unsafe extern "C" fn(u32) -> i32;
type KrunAddVirtioConsoleDefault = unsafe extern "C" fn(u32, i32, i32, i32) -> i32;
type KrunAddNetUnixstream =
    unsafe extern "C" fn(u32, *const c_char, i32, *const c_void, u32, u8) -> i32;
type KrunSetWorkdir = unsafe extern "C" fn(u32, *const c_char) -> i32;
type KrunSetExec =
    unsafe extern "C" fn(u32, *const c_char, *const *const c_char, *const *const c_char) -> i32;
type KrunStartEnter = unsafe extern "C" fn(u32) -> i32;

pub(crate) struct DynamicLibkrunApi {
    handle: *mut c_void,
    set_log_level: KrunSetLogLevel,
    init_log: Option<KrunInitLog>,
    create_ctx: KrunCreateCtx,
    free_ctx: KrunFreeCtx,
    set_vm_config: KrunSetVmConfig,
    set_root: KrunSetRoot,
    add_disk: KrunAddDisk,
    disable_implicit_console: KrunDisableImplicitConsole,
    add_virtio_console_default: KrunAddVirtioConsoleDefault,
    add_net_unixstream: Option<KrunAddNetUnixstream>,
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

        let api = unsafe {
            // SAFETY: symbols are resolved from a successfully opened libkrun handle and
            // transmuted to signatures verified against the local libkrun 1.18 header.
            Self {
                handle,
                set_log_level: std::mem::transmute::<*mut c_void, KrunSetLogLevel>(load_symbol(
                    handle,
                    "krun_set_log_level",
                )?),
                init_log: load_optional_symbol(handle, "krun_init_log")
                    .map(|symbol| std::mem::transmute::<*mut c_void, KrunInitLog>(symbol)),
                create_ctx: std::mem::transmute::<*mut c_void, KrunCreateCtx>(load_symbol(
                    handle,
                    "krun_create_ctx",
                )?),
                free_ctx: std::mem::transmute::<*mut c_void, KrunFreeCtx>(load_symbol(
                    handle,
                    "krun_free_ctx",
                )?),
                set_vm_config: std::mem::transmute::<*mut c_void, KrunSetVmConfig>(load_symbol(
                    handle,
                    "krun_set_vm_config",
                )?),
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
                add_net_unixstream: load_optional_symbol(handle, "krun_add_net_unixstream")
                    .map(|symbol| std::mem::transmute::<*mut c_void, KrunAddNetUnixstream>(symbol)),
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
pub(crate) fn planned_libkrun_load_order(override_value: Option<&str>) -> Vec<String> {
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

    fn init_log(&mut self, level: u32) -> Result<i32> {
        if let Some(init_log) = self.init_log {
            // SAFETY: optional function pointer resolved from libkrun with upstream-compatible signature.
            return Ok(unsafe {
                init_log(
                    LOFTD_LIBKRUN_LOG_TARGET_STDERR_FD,
                    level,
                    LOFTD_LIBKRUN_LOG_STYLE_NEVER,
                    LOFTD_LIBKRUN_LOG_OPTION_NO_ENV,
                )
            });
        }
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

    fn add_net_unixstream(&mut self, ctx_id: u32, socket_path: &Path) -> Result<i32> {
        let add_net_unixstream = self.add_net_unixstream.ok_or_else(|| {
            anyhow!("libkrun passt setup failed: krun_add_net_unixstream symbol is unavailable")
        })?;
        let socket_path = path_cstring(socket_path)?;
        // SAFETY: C string lives for the duration of the call. The net config pointer is NULL
        // because this slice does not expose port forwards; compat feature flags are disabled.
        Ok(unsafe {
            add_net_unixstream(
                ctx_id,
                socket_path.as_ptr(),
                -1,
                std::ptr::null(),
                LOFTD_LIBKRUN_COMPAT_NET_FEATURES,
                0,
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

unsafe fn load_optional_symbol(handle: *mut c_void, name: &str) -> Option<*mut c_void> {
    let c_name = CString::new(name).ok()?;
    // SAFETY: handle is an open dlopen handle and c_name is NUL-terminated.
    let symbol = unsafe { libc::dlsym(handle, c_name.as_ptr()) };
    if symbol.is_null() { None } else { Some(symbol) }
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
