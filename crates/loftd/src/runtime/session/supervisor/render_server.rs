//! Standalone sandboxed `virgl_render_server` runner for the venus `--gpu=drm`
//! path.
//!
//! The venus render server runs as a separate host process forked by the loftd
//! launcher *before* the network-enter helper, with its own landlock +
//! no_new_privs + seccomp sandbox. It connects back to the in-process
//! virglrenderer inside libkrun over a `SOCK_SEQPACKET` socketpair whose parent
//! end is delivered to the VM worker through the `LOFTD_RENDER_SERVER_FD`
//! environment variable (the fd survives the helper exec and the VM-worker
//! fork because the socketpair fds are not CLOEXEC).

use std::ffi::{CString, OsString};
use std::os::fd::RawFd;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use tracing::warn;

use crate::runtime::host_tools::{
    RuntimeTool, package_render_server_seccomp_policy_path_for_exe, runtime_tool_program,
};
use crate::runtime::landlock::apply_render_server_rules;
use crate::runtime::seccomp::{
    CompiledEnforcePolicy, apply_compiled_enforce_policy, compile_enforce_policy,
};
use crate::runtime::vm::libkrun::RENDER_SERVER_FD_ENV;

pub(crate) const RENDER_SERVER_EXEC_PATH_ENV: &str = "RENDER_SERVER_EXEC_PATH";
pub(crate) const RENDER_SERVER_LD_LIBRARY_PATH_ENV: &str = "LD_LIBRARY_PATH";
pub(crate) const RENDER_SERVER_VK_DRIVER_FILES_ENV: &str = "VK_DRIVER_FILES";

const MESA_LIBDIR_ENV: &str = "LOFTD_MESA_LIBDIR";
const MESA_ICD_ENV: &str = "LOFTD_MESA_ICD";
const VULKAN_LOADER_LIBDIR_ENV: &str = "LOFTD_VULKAN_LOADER_LIBDIR";
const RENDER_SERVER_POLICY_OVERRIDE_ENV: &str = "LOFTD_RENDER_SERVER_POLICY";

const DEV_DRI_PATH: &str = "/dev/dri";

/// Host store paths the venus render server loads, resolved from the loftd
/// package closure by the launcher wrapper.
#[derive(Debug, Clone)]
pub(crate) struct RenderServerEnv {
    pub(crate) exec_path: PathBuf,
    pub(crate) mesa_lib_dir: PathBuf,
    pub(crate) mesa_icd: PathBuf,
    pub(crate) vulkan_loader_lib_dir: PathBuf,
}

impl RenderServerEnv {
    pub(crate) fn resolve() -> Result<Self> {
        let exec_path = PathBuf::from(runtime_tool_program(RuntimeTool::VirglRenderServer));
        if !exec_path.is_file() {
            bail!(
                "render server executable '{}' does not exist; install loftd's packaged virgl_render_server",
                exec_path.display()
            );
        }
        Ok(Self {
            exec_path,
            mesa_lib_dir: required_env_path(MESA_LIBDIR_ENV, "mesa library directory")?,
            mesa_icd: required_env_path(MESA_ICD_ENV, "mesa Vulkan ICD")?,
            vulkan_loader_lib_dir: required_env_path(
                VULKAN_LOADER_LIBDIR_ENV,
                "vulkan-loader library directory",
            )?,
        })
    }

    fn ld_library_path(&self) -> OsString {
        // The vulkan loader must find libvulkan_radeon.so from the mesa lib
        // dir; the loader itself is dlopen'd by virgl_render_server by soname.
        let mut value = self.vulkan_loader_lib_dir.as_os_str().to_os_string();
        value.push(":");
        value.push(self.mesa_lib_dir.as_os_str());
        value
    }

    /// The runner's own environment, exported to the exec'd process.
    pub(crate) fn env_vars(&self) -> Vec<(OsString, OsString)> {
        vec![
            (
                OsString::from(RENDER_SERVER_EXEC_PATH_ENV),
                self.exec_path.clone().into_os_string(),
            ),
            (
                OsString::from(RENDER_SERVER_LD_LIBRARY_PATH_ENV),
                self.ld_library_path(),
            ),
            (
                OsString::from(RENDER_SERVER_VK_DRIVER_FILES_ENV),
                self.mesa_icd.clone().into_os_string(),
            ),
        ]
    }

    /// RO+Execute landlock roots: the Nix store path of each dependency, and
    /// the device node the render server renders through.
    pub(crate) fn store_roots(&self) -> Vec<PathBuf> {
        // Nix store layout: <store>/lib/... and <store>/libexec/...; the
        // package symlink for the render server binary is canonicalized so the
        // landlock rules cover the real virglrenderer store root the exec
        // resolves to, not just the loftd helper dir.
        let mut roots = vec![
            self.mesa_lib_dir.parent().map(Path::to_path_buf),
            self.vulkan_loader_lib_dir.parent().map(Path::to_path_buf),
        ];
        if let Ok(exec) = std::fs::canonicalize(&self.exec_path)
            && let Some(store_root) = exec.parent().and_then(Path::parent)
        {
            roots.push(Some(store_root.to_path_buf()));
        }
        roots.into_iter().flatten().collect()
    }
}

fn required_env_path(name: &str, description: &str) -> Result<PathBuf> {
    std::env::var_os(name)
        .map(PathBuf::from)
        .ok_or_else(|| {
            anyhow!(
                "{description} is not set; expected environment variable {name} (set by the loftd package wrapper)"
            )
        })
}

/// In-flight render-server runner owned by the loftd launcher process.
///
/// Dropping the guard terminates the runner and closes the launcher's copy of
/// the parent socketpair end; the VM worker keeps its own inherited copy.
pub(crate) struct RenderServerGuard {
    pid: libc::pid_t,
    parent_fd: RawFd,
    pub(crate) env: RenderServerEnv,
}

impl RenderServerGuard {
    fn terminate_runner(&mut self) {
        if self.pid > 0 {
            // SAFETY: pid is the launcher's direct fork child; ECHILD is fine
            // if the runner already exited.
            unsafe {
                libc::kill(self.pid, libc::SIGTERM);
                libc::waitpid(self.pid, std::ptr::null_mut(), 0);
            }
            self.pid = -1;
        }
    }
}

impl Drop for RenderServerGuard {
    fn drop(&mut self) {
        self.terminate_runner();
        // SAFETY: parent_fd is the launcher's own socketpair end.
        unsafe {
            libc::close(self.parent_fd);
        }
    }
}

pub(crate) fn spawn_render_server() -> Result<RenderServerGuard> {
    let env = RenderServerEnv::resolve()?;
    let policy_path = render_server_policy_path()?;
    let seccomp = compile_enforce_policy(&policy_path)?;

    let mut fds = [0; 2];
    // SAFETY: socketpair writes the two connected fds into fds; SOCK_SEQPACKET
    // preserves message boundaries between the render server and the in-process
    // virglrenderer, and the fds are deliberately not CLOEXEC so they survive
    // the helper exec and the VM-worker fork.
    if unsafe { libc::socketpair(libc::AF_UNIX, libc::SOCK_SEQPACKET, 0, fds.as_mut_ptr()) } != 0 {
        return Err(std::io::Error::last_os_error())
            .context("failed to create render-server socketpair");
    }
    let (parent_fd, child_fd) = (fds[0], fds[1]);

    // SAFETY: fork returns 0 in the child and the child pid in the parent.
    let pid = unsafe { libc::fork() };
    if pid < 0 {
        let err = std::io::Error::last_os_error();
        // SAFETY: closing the launcher's own socketpair fds on the error path.
        unsafe {
            libc::close(parent_fd);
            libc::close(child_fd);
        }
        return Err(err).context("failed to fork render-server runner");
    }
    if pid == 0 {
        run_render_server_child(&env, &seccomp, child_fd, parent_fd);
    }

    // SAFETY: the runner child owns the other end of the socketpair.
    unsafe {
        libc::close(child_fd);
    }
    // SAFETY: single-threaded before the helper spawn; the env var carries the
    // parent end of the socketpair to the VM worker through the helper exec.
    unsafe { std::env::set_var(RENDER_SERVER_FD_ENV, parent_fd.to_string()) };
    Ok(RenderServerGuard {
        pid,
        parent_fd,
        env,
    })
}

fn run_render_server_child(
    env: &RenderServerEnv,
    seccomp: &CompiledEnforcePolicy,
    child_fd: RawFd,
    parent_fd: RawFd,
) -> ! {
    // SAFETY: closing the launcher's end of the socketpair in the child.
    unsafe {
        libc::close(parent_fd);
    }

    // Landlock is defense in depth here; if the running kernel lacks support we
    // still enforce the seccomp allowlist below.
    if let Err(err) = apply_render_server_rules(&env.store_roots(), Path::new(DEV_DRI_PATH)) {
        warn!(
            error = %err,
            "render server landlock rules not applied; seccomp filter still enforced"
        );
    }

    for (name, value) in env.env_vars() {
        // SAFETY: setenv happens before the seccomp filter is installed.
        unsafe { std::env::set_var(name, value) };
    }

    if let Err(err) = apply_compiled_enforce_policy(seccomp) {
        eprintln!("loftd render server: {err:#}");
        // SAFETY: _exit is async-signal-safe and skips destructors.
        unsafe { libc::_exit(1) };
    }

    let exec_path = CString::new(env.exec_path.as_os_str().as_bytes())
        .expect("render server exec path contains no NUL byte");
    let arg0 = CString::new("virgl_render_server").expect("static argv string has no NUL byte");
    let socket_option = CString::new(format!("--socket-fd={child_fd}"))
        .expect("socket-fd option contains no NUL byte");
    let argv = [arg0.as_ptr(), socket_option.as_ptr(), std::ptr::null()];
    // SAFETY: execv with a NUL-terminated path and argv array; only returns on
    // failure.
    unsafe {
        libc::execv(exec_path.as_ptr(), argv.as_ptr());
    }
    eprintln!(
        "loftd render server: failed to exec '{}': {}",
        env.exec_path.display(),
        std::io::Error::last_os_error()
    );
    // SAFETY: _exit is async-signal-safe and skips destructors.
    unsafe { libc::_exit(1) };
}

fn render_server_policy_path() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os(RENDER_SERVER_POLICY_OVERRIDE_ENV)
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
    {
        return Ok(path);
    }
    std::env::current_exe()
        .ok()
        .and_then(|exe| package_render_server_seccomp_policy_path_for_exe(&exe))
        .ok_or_else(|| anyhow!("cannot resolve the render-server seccomp policy path"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env() -> RenderServerEnv {
        RenderServerEnv {
            exec_path: PathBuf::from(
                "/nix/store/vgl-virglrenderer-1.3.0/libexec/virgl_render_server",
            ),
            mesa_lib_dir: PathBuf::from("/nix/store/mesa-mesa-26.1.8/lib"),
            mesa_icd: PathBuf::from(
                "/nix/store/mesa-mesa-26.1.8/share/vulkan/icd.d/radeon_icd.x86_64.json",
            ),
            vulkan_loader_lib_dir: PathBuf::from("/nix/store/vk-vulkan-loader-1.4.341.0/lib"),
        }
    }

    #[test]
    fn render_server_env_vars_export_the_runner_environment() {
        let vars = env().env_vars();
        assert!(vars.contains(&(
            OsString::from(RENDER_SERVER_EXEC_PATH_ENV),
            OsString::from("/nix/store/vgl-virglrenderer-1.3.0/libexec/virgl_render_server")
        )));
        assert!(vars.contains(&(
            OsString::from(RENDER_SERVER_LD_LIBRARY_PATH_ENV),
            OsString::from(
                "/nix/store/vk-vulkan-loader-1.4.341.0/lib:/nix/store/mesa-mesa-26.1.8/lib"
            )
        )));
        assert!(vars.contains(&(
            OsString::from(RENDER_SERVER_VK_DRIVER_FILES_ENV),
            OsString::from("/nix/store/mesa-mesa-26.1.8/share/vulkan/icd.d/radeon_icd.x86_64.json")
        )));
    }

    #[test]
    fn render_server_store_roots_are_derived_from_lib_and_libexec_parents() {
        let temp = tempfile::tempdir().expect("tempdir");
        let base = temp.path().canonicalize().expect("canonicalize tempdir");
        let mesa_root = base.join("mesa-26.1.8");
        let vulkan_root = base.join("vulkan-loader-1.4.341.0");
        let vgl_root = base.join("vgl-virglrenderer-1.3.0");
        let loftd_root = base.join("loftd-agentbox");
        let vgl_bin = vgl_root.join("libexec/virgl_render_server");
        std::fs::create_dir_all(vgl_bin.parent().expect("vgl libexec dir")).expect("mkdir vgl");
        std::fs::write(&vgl_bin, b"x").expect("write vgl binary");
        std::fs::create_dir_all(mesa_root.join("lib")).expect("mkdir mesa lib");
        std::fs::create_dir_all(vulkan_root.join("lib")).expect("mkdir vulkan lib");
        std::fs::create_dir_all(loftd_root.join("libexec/loftd-helpers"))
            .expect("mkdir loftd helpers");
        std::os::unix::fs::symlink(
            &vgl_bin,
            loftd_root.join("libexec/loftd-helpers/virgl_render_server"),
        )
        .expect("symlink render server");

        let env = RenderServerEnv {
            exec_path: loftd_root.join("libexec/loftd-helpers/virgl_render_server"),
            mesa_lib_dir: mesa_root.join("lib"),
            mesa_icd: mesa_root.join("share/vulkan/icd.d/radeon_icd.x86_64.json"),
            vulkan_loader_lib_dir: vulkan_root.join("lib"),
        };
        let roots = env.store_roots();
        assert!(roots.contains(&mesa_root));
        assert!(roots.contains(&vulkan_root));
        // The exec symlink is canonicalized to the real virglrenderer store
        // root, not the loftd helper dir it lives in.
        assert!(roots.contains(&vgl_root));
        assert!(!roots.contains(&loftd_root));
    }
}
