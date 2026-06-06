use anyhow::{Context, Result, anyhow, bail};
use std::ffi::CString;
use std::fs;
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const STARTUP_TIMEOUT: Duration = Duration::from_secs(5);
const STARTUP_POLL: Duration = Duration::from_millis(25);

#[cfg(test)]
use crate::runtime::launch::config::NetworkMode;
pub(crate) mod addresses;
mod pid;
mod plan;

pub(crate) use pid::{
    cleanup_pid, passt_pid_pipe, read_passt_pid, status_exit_code, wait_pid, write_passt_pid,
};
#[cfg(test)]
use plan::PASST_SOCKET_FILE;
pub(crate) use plan::{PASST_PROGRAM, PASTA_PROGRAM, passt_plan, pasta_plan};

pub(crate) struct NetworkManagerSession {
    holder: HolderGuard,
    pasta: ManagedChild,
    passt_pid: Option<libc::pid_t>,
}

impl NetworkManagerSession {
    pub(crate) fn start(task_state_dir: &Path) -> Result<Self> {
        fs::create_dir_all(task_state_dir).with_context(|| {
            format!(
                "failed to create loftd network task state dir '{}'",
                task_state_dir.display()
            )
        })?;
        ensure_executable_available(PASTA_PROGRAM)?;
        let holder = spawn_netns_holder()?;
        let plan = pasta_plan(holder.pid());
        let mut pasta = ManagedChild::spawn(plan.command(), "pasta")?;
        wait_for_stable_child(&mut pasta).with_context(|| {
            format!(
                "pasta failed to initialize loftd network namespace '{}'",
                plan.args.join(" ")
            )
        })?;
        Ok(Self {
            holder,
            pasta,
            passt_pid: None,
        })
    }

    pub(crate) fn holder_pid(&self) -> libc::pid_t {
        self.holder.pid()
    }

    pub(crate) fn set_passt_pid(&mut self, pid: Option<libc::pid_t>) {
        self.passt_pid = pid;
    }

    pub(crate) fn cleanup(&mut self) {
        if let Some(pid) = self.passt_pid.take() {
            pid::kill_and_wait_pid(pid);
        }
        self.pasta.kill_and_wait();
        self.holder.kill_and_wait();
    }
}

impl Drop for NetworkManagerSession {
    fn drop(&mut self) {
        self.cleanup();
    }
}

pub(crate) struct PasstWorkerSession {
    child: ManagedChild,
    socket: PathBuf,
}

impl PasstWorkerSession {
    pub(crate) fn start(task_state_dir: &Path) -> Result<Self> {
        ensure_executable_available(PASST_PROGRAM)?;
        let plan = passt_plan(task_state_dir);
        let socket = plan
            .socket
            .clone()
            .ok_or_else(|| anyhow!("internal passt plan missing socket"))?;
        let _ = fs::remove_file(&socket);
        let mut child = ManagedChild::spawn(plan.command(), "passt")?;
        wait_for_socket(&socket, &mut child).with_context(|| {
            format!(
                "passt failed to initialize loftd unix socket '{}'",
                socket.display()
            )
        })?;
        Ok(Self { child, socket })
    }

    pub(crate) fn pid(&self) -> libc::pid_t {
        self.child.pid()
    }

    pub(crate) fn socket(&self) -> &Path {
        &self.socket
    }
}

impl Drop for PasstWorkerSession {
    fn drop(&mut self) {
        self.child.kill_and_wait();
        let _ = fs::remove_file(&self.socket);
    }
}

struct ManagedChild {
    child: Child,
    name: &'static str,
}

impl ManagedChild {
    fn spawn(mut command: Command, name: &'static str) -> Result<Self> {
        let child = command
            .spawn()
            .with_context(|| format!("failed to start loftd {name}; is {name} on PATH?"))?;
        Ok(Self { child, name })
    }

    fn pid(&self) -> libc::pid_t {
        self.child.id() as libc::pid_t
    }

    fn has_exited(&mut self) -> Result<Option<std::process::ExitStatus>> {
        self.child
            .try_wait()
            .with_context(|| format!("failed to poll loftd {}", self.name))
    }

    fn kill_and_wait(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for ManagedChild {
    fn drop(&mut self) {
        self.kill_and_wait();
    }
}

pub(crate) fn enter_netns(holder_pid: libc::pid_t) -> Result<()> {
    let path = format!("/proc/{holder_pid}/ns/net");
    let c_path = CString::new(path.clone())?;
    // SAFETY: open is called with a NUL-terminated path and read-only flags.
    let fd = unsafe { libc::open(c_path.as_ptr(), libc::O_RDONLY | libc::O_CLOEXEC) };
    if fd < 0 {
        bail!(
            "failed to open loftd target network namespace '{}': {}",
            path,
            std::io::Error::last_os_error()
        );
    }
    // SAFETY: fd is owned by this function after successful open.
    let fd = unsafe { OwnedFd::from_raw_fd(fd) };
    // SAFETY: fd refers to a Linux network namespace and the clone flag selects CLONE_NEWNET.
    let rc = unsafe { libc::setns(fd.as_raw_fd(), libc::CLONE_NEWNET) };
    if rc < 0 {
        bail!(
            "failed to enter loftd target network namespace '{}': {}",
            path,
            std::io::Error::last_os_error()
        );
    }
    Ok(())
}

#[cfg(test)]
fn passt_socket_for_mode(mode: NetworkMode, task_state_dir: &Path) -> Option<PathBuf> {
    match mode {
        NetworkMode::Tsi => None,
        NetworkMode::Passt => Some(task_state_dir.join(PASST_SOCKET_FILE)),
    }
}

fn ensure_executable_available(program: &str) -> Result<()> {
    let status = Command::new(program)
        .arg("--help")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .with_context(|| format!("loftd requires {program} on PATH for host alias networking"))?;
    if status.success() {
        Ok(())
    } else {
        bail!("loftd requires executable {program}, but `{program} --help` failed with {status}")
    }
}

struct HolderGuard {
    pid: libc::pid_t,
}

impl HolderGuard {
    fn pid(&self) -> libc::pid_t {
        self.pid
    }

    fn kill_and_wait(&mut self) {
        if self.pid > 0 {
            pid::kill_and_wait_pid(self.pid);
            self.pid = -1;
        }
    }
}

impl Drop for HolderGuard {
    fn drop(&mut self) {
        self.kill_and_wait();
    }
}

fn spawn_netns_holder() -> Result<HolderGuard> {
    let (read_fd, write_fd) = pid::pipe_cloexec("loftd network namespace holder readiness pipe")?;
    // SAFETY: fork is used to create a simple namespace holder process. The child either
    // unshares and pauses or exits immediately; no Rust references are shared back.
    let pid = unsafe { libc::fork() };
    if pid < 0 {
        bail!(
            "failed to fork loftd network namespace holder: {}",
            std::io::Error::last_os_error()
        );
    }
    if pid == 0 {
        drop(read_fd);
        // SAFETY: child is isolated; unshare creates the target netns held by pause().
        let rc = unsafe { libc::unshare(libc::CLONE_NEWNET) };
        if rc < 0 {
            eprintln!(
                "loftd internal: failed to unshare target network namespace: {}",
                std::io::Error::last_os_error()
            );
            std::process::exit(1);
        }
        if let Err(err) = write_ready_byte(write_fd) {
            eprintln!(
                "loftd internal: failed to report target network namespace readiness: {err:#}"
            );
            std::process::exit(1);
        }
        loop {
            // SAFETY: pause waits until the manager terminates this holder.
            unsafe { libc::pause() };
        }
    }
    drop(write_fd);
    wait_for_holder_ready(read_fd, pid)?;
    Ok(HolderGuard { pid })
}

fn write_ready_byte(fd: OwnedFd) -> Result<()> {
    let mut file = fs::File::from(fd);
    file.write_all(b"1")
        .context("failed to write holder readiness byte")
}

fn wait_for_holder_ready(fd: OwnedFd, pid: libc::pid_t) -> Result<()> {
    let mut file = fs::File::from(fd);
    let mut byte = [0; 1];
    match file.read(&mut byte) {
        Ok(1) => Ok(()),
        Ok(_) => {
            let mut status = 0;
            // SAFETY: best-effort reap of the holder after readiness EOF.
            let _ = unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) };
            bail!("loftd network namespace holder exited before readiness")
        }
        Err(err) => Err(err).context("failed to read loftd network namespace holder readiness"),
    }
}

fn wait_for_socket(socket: &Path, child: &mut ManagedChild) -> Result<()> {
    wait_until_ready(child, || socket.exists()).map(|_| ())
}

fn wait_for_stable_child(child: &mut ManagedChild) -> Result<()> {
    let deadline = Instant::now() + STARTUP_POLL * 4;
    while Instant::now() < deadline {
        if let Some(status) = child.has_exited()? {
            bail!("loftd {} exited before readiness with {status}", child.name);
        }
        thread::sleep(STARTUP_POLL);
    }
    Ok(())
}

fn wait_until_ready(child: &mut ManagedChild, mut ready: impl FnMut() -> bool) -> Result<()> {
    let deadline = Instant::now() + STARTUP_TIMEOUT;
    while Instant::now() < deadline {
        if ready() {
            return Ok(());
        }
        if let Some(status) = child.has_exited()? {
            bail!("loftd {} exited before readiness with {status}", child.name);
        }
        thread::sleep(STARTUP_POLL);
    }
    bail!("loftd {} did not become ready within 5s", child.name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pasta_plan_matches_podman_like_host_alias_contract() {
        let plan = pasta_plan(1234);

        assert_eq!(plan.program, "pasta");
        assert!(plan.args.contains(&"--foreground".to_owned()));
        assert!(
            plan.args
                .windows(2)
                .any(|w| w == ["--map-guest-addr", "169.254.1.2"])
        );
        assert!(
            plan.args
                .windows(2)
                .any(|w| w == ["--dns-forward", "169.254.1.1"])
        );
        assert!(
            plan.args
                .windows(2)
                .any(|w| w == ["--netns", "/proc/1234/ns/net"])
        );
        assert!(!plan.args.contains(&"--pid".to_owned()));
        assert!(plan.args.windows(2).any(|w| w == ["-t", "none"]));
        assert!(plan.args.windows(2).any(|w| w == ["-u", "none"]));
        assert!(plan.args.contains(&"--no-map-gw".to_owned()));
    }

    #[test]
    fn passt_plan_uses_unix_socket_and_no_publish_defaults() {
        let plan = passt_plan(Path::new("/tmp/task"));

        assert_eq!(plan.program, "passt");
        assert!(plan.args.contains(&"--foreground".to_owned()));
        let socket = plan.socket.as_ref().expect("passt socket");
        assert!(socket.starts_with("/tmp"));
        assert!(socket.to_string_lossy().contains("loftd-"));
        assert!(socket.to_string_lossy().ends_with("-passt.sock"));
        assert!(
            plan.args
                .windows(2)
                .any(|w| w == ["--socket", socket.to_string_lossy().as_ref()])
        );
        assert!(
            plan.args
                .windows(2)
                .any(|w| w == ["--map-guest-addr", "169.254.1.2"])
        );
        assert!(
            plan.args
                .windows(2)
                .any(|w| w == ["--dns-forward", "169.254.1.1"])
        );
        assert!(plan.args.contains(&"--one-off".to_owned()));
        assert!(!plan.args.contains(&"-T".to_owned()));
        assert!(!plan.args.contains(&"-U".to_owned()));
    }

    #[test]
    fn passt_socket_is_only_planned_for_passt_mode() {
        assert_eq!(
            passt_socket_for_mode(NetworkMode::Tsi, Path::new("/tmp/task")),
            None
        );
        assert_eq!(
            passt_socket_for_mode(NetworkMode::Passt, Path::new("/tmp/task")),
            Some(Path::new("/tmp/task/passt.sock").to_path_buf())
        );
    }
}
