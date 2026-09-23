//! Workspace bind mount contribution.
//!
//! This file owns only the existing workspace mount contribution; it does not
//! define new mount policy or validation behavior.

use std::path::Path;

use crate::runtime::launch::config::{BindMount, WORKSPACE_TAG, WORKSPACE_TARGET};

pub(crate) fn bind_mount(workspace_dir: &Path) -> BindMount {
    super::bind_mount(workspace_dir, WORKSPACE_TAG, WORKSPACE_TARGET)
}
