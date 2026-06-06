//! Runtime orchestration split by launch dataflow ownership.
//!
//! - `launch` resolves host intent and assembles the helper/libkrun launch contract.
//! - `session` owns the host-side launch workflow and helper process supervision.
//! - `vm` owns libkrun, prepared-root, and VM network mechanics.

use anyhow::Result;
use std::ffi::OsString;
use std::process::ExitCode;

use crate::cli::RuntimeOptions;

pub(crate) mod launch;
pub(crate) mod session;
pub(crate) mod vm;

pub(crate) fn run(options: RuntimeOptions) -> Result<ExitCode> {
    session::run(options)
}

pub(crate) fn run_internal(args: Vec<OsString>) -> Result<()> {
    session::run_internal(args)
}
