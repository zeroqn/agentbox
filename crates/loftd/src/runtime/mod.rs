//! Runtime orchestration split by launch dataflow ownership.
//!
//! - `launch` resolves host intent and assembles the helper/libkrun launch contract.
//! - `session` owns the host-side launch workflow and helper process supervision.
//! - `vm` owns libkrun, prepared-root, and VM network mechanics.

use anyhow::Result;
use std::ffi::OsString;
use std::process::ExitCode;
use std::time::Instant;

use crate::cli::RuntimeOptions;

pub(crate) mod launch;
pub(crate) mod session;
pub(crate) mod vm;

#[derive(Debug, Clone, Copy)]
pub(crate) struct RuntimeProfileScope {
    started_at: Instant,
}

impl RuntimeProfileScope {
    pub(crate) fn from_started_at(started_at: Instant) -> Self {
        Self { started_at }
    }

    pub(crate) fn started_at(self) -> Instant {
        self.started_at
    }
}

pub(crate) fn run(options: RuntimeOptions, profile_scope: RuntimeProfileScope) -> Result<ExitCode> {
    session::run(options, profile_scope)
}

pub(crate) fn run_internal(args: Vec<OsString>) -> Result<()> {
    session::run_internal(args)
}
