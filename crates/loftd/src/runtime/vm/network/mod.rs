use anyhow::{Context, Result, bail};
use std::ffi::CString;
use std::fs;
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use crate::runtime::launch::config::NetworkMode;
use crate::runtime::publish::tsi_pasta_tcp_forwards;

const STARTUP_POLL: Duration = Duration::from_millis(25);

pub(crate) mod addresses;
mod pid;
mod plan;

pub(crate) use pid::{cleanup_pid, status_exit_code, wait_pid};
pub(crate) use plan::{PASST_PROGRAM, PASTA_PROGRAM, passt_plan, pasta_plan};

pub(crate) struct NetworkManagerSession {
    holder: HolderGuard,
    pasta: ManagedChild,
}

impl NetworkManagerSession {
    pub(crate) fn start(
        task_state_dir: &Path,
        network_mode: NetworkMode,
        publish: &[String],
    ) -> Result<Self> {
        fs::create_dir_all(task_state_dir).with_context(|| {
            format!(
                "failed to create loftd network task state dir '{}'",
                task_state_dir.display()
            )
        })?;
        ensure_executable_available(PASTA_PROGRAM)?;
        let tcp_forwards = pasta_tcp_forwards_for(network_mode, publish)?;
        let holder = spawn_netns_holder()?;
        let plan = pasta_plan(holder.pid(), &tcp_forwards);
        let mut pasta = ManagedChild::spawn(plan.command(), "pasta")?;
        wait_for_stable_child(&mut pasta).with_context(|| {
            format!(
                "pasta failed to initialize loftd network namespace '{}'",
                plan.args.join(" ")
            )
        })?;
        Ok(Self { holder, pasta })
    }

    pub(crate) fn holder_pid(&self) -> libc::pid_t {
        self.holder.pid()
    }

    pub(crate) fn cleanup(&mut self) {
        self.pasta.kill_and_wait();
        self.holder.kill_and_wait();
    }
}

impl Drop for NetworkManagerSession {
    fn drop(&mut self) {
        self.cleanup();
    }
}

fn pasta_tcp_forwards_for(network_mode: NetworkMode, publish: &[String]) -> Result<Vec<String>> {
    match network_mode {
        NetworkMode::Tsi => tsi_pasta_tcp_forwards(publish),
        NetworkMode::Passt => Ok(Vec::new()),
    }
}

pub(crate) struct PasstWorkerSession {
    child: ManagedChild,
    passt_fd: OwnedFd,
}

impl PasstWorkerSession {
    pub(crate) fn start(publish: &[String]) -> Result<Self> {
        ensure_executable_available(PASST_PROGRAM)?;
        let (passt_fd, child_fd) = passt_socketpair()?;
        let plan = passt_plan(child_fd.as_raw_fd(), publish)?;
        let mut command = plan.command();
        let passt_fd_raw = passt_fd.as_raw_fd();
        // SAFETY: this closure only invokes async-signal-safe `close` in the child
        // after fork and before exec so passt does not inherit libkrun's socket fd.
        unsafe {
            command.pre_exec(move || {
                let rc = libc::close(passt_fd_raw);
                if rc < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let mut child = ManagedChild::spawn(command, "passt")?;
        drop(child_fd);
        wait_for_stable_child(&mut child).with_context(|| {
            format!(
                "passt failed to initialize loftd fd backend '{}'",
                plan.args.join(" ")
            )
        })?;
        Ok(Self { child, passt_fd })
    }

    pub(crate) fn fd(&self) -> i32 {
        self.passt_fd.as_raw_fd()
    }
}

impl Drop for PasstWorkerSession {
    fn drop(&mut self) {
        self.child.kill_and_wait();
    }
}

fn passt_socketpair() -> Result<(OwnedFd, OwnedFd)> {
    let mut fds = [-1; 2];
    // SAFETY: socketpair writes two file descriptors into the provided array on success.
    let rc = unsafe { libc::socketpair(libc::AF_UNIX, libc::SOCK_STREAM, 0, fds.as_mut_ptr()) };
    if rc < 0 {
        bail!(
            "failed to create loftd passt socketpair: {}",
            std::io::Error::last_os_error()
        );
    }
    // SAFETY: both descriptors are initialized and uniquely owned after successful socketpair.
    let parent = unsafe { OwnedFd::from_raw_fd(fds[0]) };
    // SAFETY: both descriptors are initialized and uniquely owned after successful socketpair.
    let child = unsafe { OwnedFd::from_raw_fd(fds[1]) };
    Ok((parent, child))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pasta_plan_matches_podman_like_host_alias_contract() {
        let plan = pasta_plan(1234, &[]);

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
    fn pasta_plan_emits_tsi_publish_tcp_forwards() {
        let forwards = vec!["8080:8080".to_owned(), "8443:8443".to_owned()];
        let plan = pasta_plan(1234, &forwards);

        assert!(plan.args.windows(2).any(|w| w == ["-t", "8080:8080"]));
        assert!(plan.args.windows(2).any(|w| w == ["-t", "8443:8443"]));
        assert!(!plan.args.windows(2).any(|w| w == ["-t", "none"]));
        assert!(plan.args.windows(2).any(|w| w == ["-u", "none"]));
        assert!(plan.args.windows(2).any(|w| w == ["-T", "none"]));
        assert!(plan.args.windows(2).any(|w| w == ["-U", "none"]));
    }

    #[test]
    fn tsi_mode_derives_pasta_forwards_from_publish_specs() {
        let publish = vec!["8080:80".to_owned(), "8443:443".to_owned()];

        assert_eq!(
            pasta_tcp_forwards_for(NetworkMode::Tsi, &publish).expect("TSI forwards"),
            ["8080:8080", "8443:8443"]
        );
    }

    #[test]
    fn passt_mode_keeps_pasta_inbound_closed_when_publish_is_present() {
        let publish = vec!["8080:80".to_owned(), "udp:5353:5353".to_owned()];

        assert_eq!(
            pasta_tcp_forwards_for(NetworkMode::Passt, &publish).expect("passt forwards"),
            Vec::<String>::new()
        );
    }

    #[test]
    fn tsi_mode_rejects_passt_only_publish_syntax_before_pasta_spawn() {
        let error = pasta_tcp_forwards_for(NetworkMode::Tsi, &["udp:5353:5353".to_owned()])
            .expect_err("UDP is passt-only");

        assert!(format!("{error:#}").contains("TSI publish spec"));
    }

    #[test]
    fn passt_plan_uses_fd_backend_and_no_publish_defaults() {
        let plan = passt_plan(42, &[]).expect("passt plan");

        assert_eq!(plan.program, "passt");
        assert!(plan.args.contains(&"--foreground".to_owned()));
        assert_eq!(plan.fd, Some(42));
        assert!(plan.args.windows(2).any(|w| w == ["--fd", "42"]));
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
        assert!(!plan.args.contains(&"--socket".to_owned()));
        assert!(!plan.args.contains(&"--one-off".to_owned()));
        assert!(plan.args.windows(2).any(|w| w == ["-t", "none"]));
        assert!(plan.args.windows(2).any(|w| w == ["-u", "none"]));
        assert!(!plan.args.contains(&"-T".to_owned()));
        assert!(!plan.args.contains(&"-U".to_owned()));
    }

    #[test]
    fn passt_plan_emits_tcp_publish_args() {
        let publish = vec!["8080:80".to_owned(), "tcp:8443:443".to_owned()];
        let plan = passt_plan(42, &publish).expect("passt plan");

        assert!(plan.args.windows(2).any(|w| w == ["-t", "8080:80"]));
        assert!(plan.args.windows(2).any(|w| w == ["-t", "8443:443"]));
        assert!(plan.args.windows(2).any(|w| w == ["-u", "none"]));
    }

    #[test]
    fn passt_plan_emits_udp_publish_args() {
        let publish = vec!["udp:5353:5353".to_owned()];
        let plan = passt_plan(42, &publish).expect("passt plan");

        assert!(plan.args.windows(2).any(|w| w == ["-t", "none"]));
        assert!(plan.args.windows(2).any(|w| w == ["-u", "5353:5353"]));
    }

    #[test]
    fn passt_plan_emits_mixed_publish_args() {
        let publish = vec!["8080:80".to_owned(), "udp:5353:5353".to_owned()];
        let plan = passt_plan(42, &publish).expect("passt plan");

        assert!(plan.args.windows(2).any(|w| w == ["-t", "8080:80"]));
        assert!(plan.args.windows(2).any(|w| w == ["-u", "5353:5353"]));
        assert!(!plan.args.windows(2).any(|w| w == ["-t", "none"]));
        assert!(!plan.args.windows(2).any(|w| w == ["-u", "none"]));
    }

    #[test]
    fn passt_plan_preserves_ranges_and_bind_payloads() {
        let publish = vec![
            "10000-10010:80-90".to_owned(),
            "tcp:8080:80/127.0.0.1".to_owned(),
            "udp:5353~5354:5353%eth0".to_owned(),
        ];
        let plan = passt_plan(42, &publish).expect("passt plan");

        assert!(
            plan.args
                .windows(2)
                .any(|w| w == ["-t", "10000-10010:80-90"])
        );
        assert!(
            plan.args
                .windows(2)
                .any(|w| w == ["-t", "8080:80/127.0.0.1"])
        );
        assert!(
            plan.args
                .windows(2)
                .any(|w| w == ["-u", "5353~5354:5353%eth0"])
        );
    }

    #[test]
    fn passt_plan_rejects_empty_or_unknown_publish_selectors() {
        let empty = passt_plan(42, &["tcp:".to_owned()]).expect_err("empty payload should fail");
        let unknown = passt_plan(42, &["sctp:5000:5000".to_owned()])
            .expect_err("unknown selector should fail");

        assert!(format!("{empty:#}").contains("empty"));
        assert!(format!("{unknown:#}").contains("unsupported protocol"));
    }

    #[test]
    fn passt_socketpair_returns_connected_fds() {
        use std::io::{Read as _, Write as _};
        let (parent, child) = passt_socketpair().expect("socketpair");
        let mut parent = fs::File::from(parent);
        let mut child = fs::File::from(child);

        child.write_all(b"x").expect("write child");
        let mut byte = [0_u8; 1];
        parent.read_exact(&mut byte).expect("read parent");

        assert_eq!(byte, [b'x']);
    }
}
