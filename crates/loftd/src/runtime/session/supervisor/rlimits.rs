//! Host-side resource-limit preparation for the libkrun helper.

use anyhow::{Context, Result, bail};
use std::io;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct NofileLimits {
    soft: libc::rlim_t,
    hard: libc::rlim_t,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NofileLimitAdjustment {
    Raised {
        previous_soft: libc::rlim_t,
        hard: libc::rlim_t,
    },
    AlreadyAtHard {
        soft: libc::rlim_t,
        hard: libc::rlim_t,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NofileLimitPlan {
    Raise(NofileLimits),
    AlreadyAtHard(NofileLimits),
}

trait NofileRlimitBackend {
    fn get_nofile_limits(&mut self) -> io::Result<NofileLimits>;
    fn set_nofile_limits(&mut self, limits: NofileLimits) -> io::Result<()>;
}

#[derive(Debug, Default)]
struct LibcNofileRlimitBackend;

impl NofileRlimitBackend for LibcNofileRlimitBackend {
    fn get_nofile_limits(&mut self) -> io::Result<NofileLimits> {
        let mut limits = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        // SAFETY: `limits` is a valid writable pointer and `RLIMIT_NOFILE` is a supported
        // resource on the Linux targets loftd supports.
        let rc = unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut limits) };
        if rc == 0 {
            Ok(NofileLimits {
                soft: limits.rlim_cur,
                hard: limits.rlim_max,
            })
        } else {
            Err(io::Error::last_os_error())
        }
    }

    fn set_nofile_limits(&mut self, limits: NofileLimits) -> io::Result<()> {
        let raw_limits = libc::rlimit {
            rlim_cur: limits.soft,
            rlim_max: limits.hard,
        };
        // SAFETY: `raw_limits` is a valid readable pointer and preserves the current hard
        // limit while raising only the soft limit to a value returned by `getrlimit`.
        let rc = unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &raw_limits) };
        if rc == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }
}

pub(crate) fn raise_host_nofile_soft_limit() -> Result<()> {
    let mut backend = LibcNofileRlimitBackend;
    let adjustment = raise_host_nofile_soft_limit_with(&mut backend)?;
    match adjustment {
        NofileLimitAdjustment::Raised {
            previous_soft,
            hard,
        } => tracing::debug!(
            previous_soft,
            new_soft = hard,
            hard,
            "raised host RLIMIT_NOFILE soft limit to hard limit for libkrun helper"
        ),
        NofileLimitAdjustment::AlreadyAtHard { soft, hard } => tracing::debug!(
            soft,
            hard,
            "host RLIMIT_NOFILE soft limit already matches hard limit for libkrun helper"
        ),
    }
    Ok(())
}

fn raise_host_nofile_soft_limit_with(
    backend: &mut impl NofileRlimitBackend,
) -> Result<NofileLimitAdjustment> {
    let limits = backend
        .get_nofile_limits()
        .context("failed to read host RLIMIT_NOFILE before starting libkrun helper work")?;
    match plan_nofile_soft_limit_raise(limits)? {
        NofileLimitPlan::AlreadyAtHard(limits) => Ok(NofileLimitAdjustment::AlreadyAtHard {
            soft: limits.soft,
            hard: limits.hard,
        }),
        NofileLimitPlan::Raise(requested) => {
            backend.set_nofile_limits(requested).with_context(|| {
                format!(
                    "failed to raise host RLIMIT_NOFILE soft limit from {} to hard limit {} before starting libkrun helper work",
                    limits.soft, limits.hard
                )
            })?;
            Ok(NofileLimitAdjustment::Raised {
                previous_soft: limits.soft,
                hard: limits.hard,
            })
        }
    }
}

fn plan_nofile_soft_limit_raise(limits: NofileLimits) -> Result<NofileLimitPlan> {
    if limits.soft > limits.hard {
        bail!(
            "host RLIMIT_NOFILE is invalid: soft limit {} is greater than hard limit {}",
            limits.soft,
            limits.hard
        );
    }
    if limits.soft == limits.hard {
        Ok(NofileLimitPlan::AlreadyAtHard(limits))
    } else {
        Ok(NofileLimitPlan::Raise(NofileLimits {
            soft: limits.hard,
            hard: limits.hard,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct FakeNofileBackend {
        limits: Result<NofileLimits, i32>,
        set_error: Option<i32>,
        set_calls: Vec<NofileLimits>,
    }

    impl FakeNofileBackend {
        fn with_limits(soft: libc::rlim_t, hard: libc::rlim_t) -> Self {
            Self {
                limits: Ok(NofileLimits { soft, hard }),
                set_error: None,
                set_calls: Vec::new(),
            }
        }

        fn with_get_error() -> Self {
            Self {
                limits: Err(libc::EMFILE),
                set_error: None,
                set_calls: Vec::new(),
            }
        }

        fn with_set_error(soft: libc::rlim_t, hard: libc::rlim_t) -> Self {
            Self {
                limits: Ok(NofileLimits { soft, hard }),
                set_error: Some(libc::EPERM),
                set_calls: Vec::new(),
            }
        }
    }

    impl NofileRlimitBackend for FakeNofileBackend {
        fn get_nofile_limits(&mut self) -> io::Result<NofileLimits> {
            self.limits.map_err(io::Error::from_raw_os_error)
        }

        fn set_nofile_limits(&mut self, limits: NofileLimits) -> io::Result<()> {
            self.set_calls.push(limits);
            self.set_error
                .map_or(Ok(()), |errno| Err(io::Error::from_raw_os_error(errno)))
        }
    }

    #[test]
    fn nofile_plan_raises_low_soft_limit_to_hard_limit() {
        assert_eq!(
            plan_nofile_soft_limit_raise(NofileLimits {
                soft: 1024,
                hard: 524_288,
            })
            .unwrap(),
            NofileLimitPlan::Raise(NofileLimits {
                soft: 524_288,
                hard: 524_288,
            })
        );
    }

    #[test]
    fn nofile_plan_skips_when_soft_limit_already_matches_hard_limit() {
        let limits = NofileLimits {
            soft: 524_288,
            hard: 524_288,
        };
        assert_eq!(
            plan_nofile_soft_limit_raise(limits).unwrap(),
            NofileLimitPlan::AlreadyAtHard(limits)
        );
    }

    #[test]
    fn nofile_plan_rejects_invalid_soft_limit_above_hard_limit() {
        let err = plan_nofile_soft_limit_raise(NofileLimits {
            soft: 2048,
            hard: 1024,
        })
        .expect_err("soft above hard must not produce a setrlimit request");
        assert!(format!("{err:#}").contains("soft limit 2048 is greater than hard limit 1024"));
    }

    #[test]
    fn nofile_raise_calls_setrlimit_with_hard_as_new_soft_limit() {
        let mut backend = FakeNofileBackend::with_limits(1024, 524_288);

        let adjustment = raise_host_nofile_soft_limit_with(&mut backend).unwrap();

        assert_eq!(
            adjustment,
            NofileLimitAdjustment::Raised {
                previous_soft: 1024,
                hard: 524_288,
            }
        );
        assert_eq!(
            backend.set_calls,
            vec![NofileLimits {
                soft: 524_288,
                hard: 524_288,
            }]
        );
    }

    #[test]
    fn nofile_raise_does_not_call_setrlimit_when_already_at_hard_limit() {
        let mut backend = FakeNofileBackend::with_limits(524_288, 524_288);

        let adjustment = raise_host_nofile_soft_limit_with(&mut backend).unwrap();

        assert_eq!(
            adjustment,
            NofileLimitAdjustment::AlreadyAtHard {
                soft: 524_288,
                hard: 524_288,
            }
        );
        assert!(backend.set_calls.is_empty());
    }

    #[test]
    fn nofile_raise_reports_getrlimit_failure_with_context() {
        let mut backend = FakeNofileBackend::with_get_error();

        let err = raise_host_nofile_soft_limit_with(&mut backend).unwrap_err();

        assert!(format!("{err:#}").contains("failed to read host RLIMIT_NOFILE"));
        assert!(backend.set_calls.is_empty());
    }

    #[test]
    fn nofile_raise_reports_setrlimit_failure_with_context() {
        let mut backend = FakeNofileBackend::with_set_error(1024, 524_288);

        let err = raise_host_nofile_soft_limit_with(&mut backend).unwrap_err();

        let message = format!("{err:#}");
        assert!(message.contains("failed to raise host RLIMIT_NOFILE soft limit from 1024"));
        assert!(message.contains("524288"));
        assert_eq!(
            backend.set_calls,
            vec![NofileLimits {
                soft: 524_288,
                hard: 524_288,
            }]
        );
    }
}
