use anyhow::{Context, Result};
use std::path::Path;

pub fn format_mount_arg(path: &Path, destination: &str) -> Result<String> {
    format_mount_arg_with_options(path, destination, None)
}

pub fn format_mount_arg_with_options(
    path: &Path,
    destination: &str,
    options: Option<&str>,
) -> Result<String> {
    let path = path.to_str().with_context(|| {
        format!(
            "path '{}' is not valid UTF-8 and cannot be mounted",
            path.display()
        )
    })?;

    let mut mount = format!("{path}:{destination}");
    if let Some(options) = options {
        if !options.is_empty() {
            mount.push(':');
            mount.push_str(options);
        }
    }

    Ok(mount)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    #[test]
    fn mount_arg_uses_workspace_path() {
        let mount = crate::podman::volume::format_mount_arg(
            Path::new("/tmp/project"),
            crate::CONTAINER_WORKDIR,
        )
        .expect("mount formatting should succeed");
        assert_eq!(mount, "/tmp/project:/workspace");
    }

    #[test]
    fn mount_arg_supports_options_suffix() {
        let mount = crate::podman::volume::format_mount_arg_with_options(
            Path::new("/tmp/project"),
            "/nix",
            Some("ro"),
        )
        .expect("mount formatting should succeed");
        assert_eq!(mount, "/tmp/project:/nix:ro");
    }
}
