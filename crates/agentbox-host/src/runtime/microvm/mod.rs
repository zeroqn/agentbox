use anyhow::Result;
use std::process::ExitCode;

use crate::cli::{CommonOptions, MicrovmOptions};

pub(crate) fn run(_common: CommonOptions, _options: MicrovmOptions) -> Result<ExitCode> {
    anyhow::bail!(not_implemented_message())
}

pub(crate) fn not_implemented_message() -> &'static str {
    "experimental microvm runtime is not implemented yet; this slice only adds CLI and documentation skeletons"
}
