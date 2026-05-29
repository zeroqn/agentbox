use anyhow::{Result, bail};
use std::env;
use std::process::ExitCode;

use crate::cli::{Cli, selected_image_reference};
use crate::state;

pub(crate) fn run(cli: Cli) -> Result<ExitCode> {
    let cwd = env::current_dir()?;
    let layout = state::resolve_state_layout(&cwd)?;
    let _image_cache_dir = layout.image_cache_dir();
    let _sccache_dir = layout.sccache_dir();
    let options = cli.into_runtime_options();
    let selected_image = options
        .image
        .as_deref()
        .unwrap_or_else(|| selected_image_reference(None, options.image_resolution));

    bail!(
        "loftd microvm runtime launch is not implemented yet (state root: {}, image: {})",
        layout.root_dir().display(),
        selected_image
    );
}
