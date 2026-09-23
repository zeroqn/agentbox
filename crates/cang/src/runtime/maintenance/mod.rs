pub(crate) mod container_store;

use anyhow::Result;

use crate::cli::{ContainerStoreCommand, ContainerStoreOptions};

pub(crate) fn run_container_store_command(
    command: ContainerStoreCommand,
    options: ContainerStoreOptions,
) -> Result<String> {
    container_store::run(command, options)
}
