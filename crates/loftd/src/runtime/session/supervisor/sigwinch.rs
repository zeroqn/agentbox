//! Supervisor-side SIGWINCH forwarding for detached helper process groups.

use anyhow::{Context, Result, bail};
use std::io;
use std::mem::MaybeUninit;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

const SIGWAIT_POLL: Duration = Duration::from_millis(200);
static ACTIVE_SIGWINCH_BROKER: AtomicBool = AtomicBool::new(false);

pub(crate) struct SigwinchForwarder {
    active: Arc<AtomicBool>,
    blocked_mask: Option<BlockedSigwinchMask>,
    thread: Option<JoinHandle<()>>,
}

impl SigwinchForwarder {
    pub(crate) fn start(target_pgid: u32) -> Result<Self> {
        ensure_safe_process_group(target_pgid)?;
        let blocked_mask = BlockedSigwinchMask::block_current_thread()?;
        let active = Arc::new(AtomicBool::new(true));
        let relay_active = Arc::clone(&active);
        let thread = thread::Builder::new()
            .name("loftd-sigwinch".to_owned())
            .spawn(move || relay_sigwinch_events(target_pgid, relay_active))
            .context("failed to spawn loftd SIGWINCH relay thread")?;
        Ok(Self {
            active,
            blocked_mask: Some(blocked_mask),
            thread: Some(thread),
        })
    }
}

impl Drop for SigwinchForwarder {
    fn drop(&mut self) {
        self.active.store(false, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        let _ = self.blocked_mask.take();
    }
}

struct BlockedSigwinchMask {
    previous_mask: libc::sigset_t,
}

impl BlockedSigwinchMask {
    fn block_current_thread() -> Result<Self> {
        if ACTIVE_SIGWINCH_BROKER
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            bail!("loftd SIGWINCH forwarding already has an active broker");
        }
        let signal_set = sigwinch_set()?;
        let mut previous_mask = MaybeUninit::<libc::sigset_t>::zeroed();
        let rc = unsafe {
            libc::pthread_sigmask(libc::SIG_BLOCK, &signal_set, previous_mask.as_mut_ptr())
        };
        if rc != 0 {
            ACTIVE_SIGWINCH_BROKER.store(false, Ordering::SeqCst);
            bail!(
                "failed to block SIGWINCH in loftd supervisor thread: {}",
                io::Error::from_raw_os_error(rc)
            );
        }
        Ok(Self {
            previous_mask: unsafe { previous_mask.assume_init() },
        })
    }
}

impl Drop for BlockedSigwinchMask {
    fn drop(&mut self) {
        let rc = unsafe {
            libc::pthread_sigmask(libc::SIG_SETMASK, &self.previous_mask, std::ptr::null_mut())
        };
        if rc != 0 {
            tracing::warn!(
                error = %io::Error::from_raw_os_error(rc),
                "failed to restore supervisor SIGWINCH mask"
            );
        }
        ACTIVE_SIGWINCH_BROKER.store(false, Ordering::SeqCst);
    }
}

fn relay_sigwinch_events(target_pgid: u32, active: Arc<AtomicBool>) {
    let signal_set = match sigwinch_set() {
        Ok(signal_set) => signal_set,
        Err(err) => {
            tracing::warn!(error = %err, "failed to build SIGWINCH wait set");
            return;
        }
    };
    let mut signaler = OsProcessGroupSignaler;
    loop {
        if !active.load(Ordering::Acquire) {
            return;
        }
        match wait_for_sigwinch(&signal_set, SIGWAIT_POLL) {
            Ok(SigwaitOutcome::TimedOut) => continue,
            Ok(SigwaitOutcome::Received) => {
                if let Err(err) = forward_sigwinch(target_pgid, &mut signaler) {
                    tracing::warn!(
                        pgid = target_pgid,
                        error = %err,
                        "loftd SIGWINCH relay failed"
                    );
                }
            }
            Err(err) => {
                tracing::warn!(error = %err, "loftd SIGWINCH wait failed");
                return;
            }
        }
    }
}

enum SigwaitOutcome {
    TimedOut,
    Received,
}

trait ProcessGroupSignaler {
    fn signal_process_group(&mut self, pgid: u32, signal: i32) -> Result<()>;
}

struct OsProcessGroupSignaler;

impl ProcessGroupSignaler for OsProcessGroupSignaler {
    fn signal_process_group(&mut self, pgid: u32, signal: i32) -> Result<()> {
        ensure_safe_process_group(pgid)?;
        let pgid = i32::try_from(pgid).context("process group id does not fit in i32")?;
        let rc = unsafe { libc::kill(-pgid, signal) };
        if rc == 0 {
            return Ok(());
        }
        Err(io::Error::last_os_error())
            .with_context(|| format!("failed to signal helper process group {pgid}"))
    }
}

fn forward_sigwinch(pgid: u32, signaler: &mut impl ProcessGroupSignaler) -> Result<()> {
    match signaler.signal_process_group(pgid, libc::SIGWINCH) {
        Ok(()) => Ok(()),
        Err(err) if is_esrch(&err) => Ok(()),
        Err(err) => Err(err),
    }
}

fn ensure_safe_process_group(pgid: u32) -> Result<()> {
    let pgid = i32::try_from(pgid).context("process group id does not fit in i32")?;
    if pgid <= 1 {
        bail!("refusing to signal unsafe process group id {pgid}");
    }
    Ok(())
}

fn sigwinch_set() -> Result<libc::sigset_t> {
    let mut signal_set = MaybeUninit::<libc::sigset_t>::zeroed();
    let signal_set = unsafe {
        let signal_set = signal_set.assume_init_mut();
        if libc::sigemptyset(signal_set) != 0 {
            bail!(
                "failed to initialize SIGWINCH signal set: {}",
                io::Error::last_os_error()
            );
        }
        if libc::sigaddset(signal_set, libc::SIGWINCH) != 0 {
            bail!(
                "failed to add SIGWINCH to signal set: {}",
                io::Error::last_os_error()
            );
        }
        *signal_set
    };
    Ok(signal_set)
}

fn wait_for_sigwinch(signal_set: &libc::sigset_t, timeout: Duration) -> Result<SigwaitOutcome> {
    let timeout = libc::timespec {
        tv_sec: i64::try_from(timeout.as_secs())
            .context("SIGWINCH wait timeout seconds overflow")?,
        tv_nsec: i64::from(timeout.subsec_nanos()),
    };
    let mut signal_info = MaybeUninit::<libc::siginfo_t>::zeroed();
    let rc = unsafe { libc::sigtimedwait(signal_set, signal_info.as_mut_ptr(), &timeout) };
    if rc == libc::SIGWINCH {
        return Ok(SigwaitOutcome::Received);
    }
    if rc < 0 {
        let err = io::Error::last_os_error();
        if err.raw_os_error() == Some(libc::EAGAIN) {
            return Ok(SigwaitOutcome::TimedOut);
        }
        if err.kind() == io::ErrorKind::Interrupted {
            return Ok(SigwaitOutcome::TimedOut);
        }
        return Err(err).context("failed while waiting for supervisor SIGWINCH");
    }
    bail!("unexpected signal {rc} received while waiting for SIGWINCH")
}

fn is_esrch(err: &anyhow::Error) -> bool {
    err.chain()
        .filter_map(|cause| cause.downcast_ref::<io::Error>())
        .any(|io| io.raw_os_error() == Some(libc::ESRCH))
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::anyhow;
    use std::sync::{LazyLock, Mutex};

    static SIGWINCH_BROKER_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

    #[derive(Default)]
    struct RecordingSignaler {
        calls: Vec<(u32, i32)>,
        error: Option<anyhow::Error>,
    }

    impl ProcessGroupSignaler for RecordingSignaler {
        fn signal_process_group(&mut self, pgid: u32, signal: i32) -> Result<()> {
            self.calls.push((pgid, signal));
            match self.error.take() {
                Some(err) => Err(err),
                None => Ok(()),
            }
        }
    }

    #[test]
    fn forward_sigwinch_targets_helper_process_group() {
        let mut signaler = RecordingSignaler::default();
        forward_sigwinch(123, &mut signaler).expect("forward should succeed");
        assert_eq!(signaler.calls, vec![(123, libc::SIGWINCH)]);
    }

    #[test]
    fn forward_sigwinch_ignores_esrch_exit_race() {
        let mut signaler = RecordingSignaler {
            calls: Vec::new(),
            error: Some(anyhow!(io::Error::from_raw_os_error(libc::ESRCH))),
        };
        forward_sigwinch(123, &mut signaler).expect("ESRCH should be ignored");
        assert_eq!(signaler.calls, vec![(123, libc::SIGWINCH)]);
    }

    #[test]
    fn forward_sigwinch_propagates_non_esrch_errors() {
        let mut signaler = RecordingSignaler {
            calls: Vec::new(),
            error: Some(anyhow!(io::Error::from_raw_os_error(libc::EPERM))),
        };
        let err = forward_sigwinch(123, &mut signaler).expect_err("EPERM should fail");
        assert!(format!("{err:#}").contains("Operation not permitted"));
    }

    #[test]
    fn broker_registration_rejects_second_active_broker_until_drop() {
        let _guard = SIGWINCH_BROKER_LOCK.lock().expect("signal lock");
        let first = BlockedSigwinchMask::block_current_thread().expect("first broker");
        let err = match BlockedSigwinchMask::block_current_thread() {
            Ok(mask) => {
                drop(mask);
                panic!("second broker should fail while first is active");
            }
            Err(err) => err,
        };
        assert!(format!("{err:#}").contains("already has an active broker"));
        drop(first);
        BlockedSigwinchMask::block_current_thread()
            .expect("broker registration should recover after drop");
    }
}
