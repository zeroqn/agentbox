mod task;
mod workspace;

pub(crate) use task::{derive_task_container_name, derive_task_hostname};
pub(crate) use workspace::derive_workspace_slug;
