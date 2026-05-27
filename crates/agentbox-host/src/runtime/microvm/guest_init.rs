use anyhow::{Context, Result, bail};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

const GUEST_INIT_BASENAME: &str = "agentbox-guest-init";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedGuestInit {
    pub(crate) host_path: PathBuf,
    pub(crate) guest_exec_path: String,
}

pub(crate) fn resolve_guest_init(
    task_rootfs: &Path,
    override_path: Option<&Path>,
) -> Result<ResolvedGuestInit> {
    let target = find_agentbox_guest_init(task_rootfs)?;
    if let Some(override_path) = override_path {
        copy_guest_init_override(override_path, &target)?;
    }
    Ok(ResolvedGuestInit {
        guest_exec_path: guest_path(task_rootfs, &target)?,
        host_path: target,
    })
}

pub(crate) fn find_agentbox_guest_init(task_rootfs: &Path) -> Result<PathBuf> {
    let store = task_rootfs.join("nix/store");
    let mut matches = Vec::new();
    if store.is_dir() {
        for entry in fs::read_dir(&store).with_context(|| {
            format!(
                "failed to read microvm task rootfs store '{}'",
                store.display()
            )
        })? {
            let entry = entry?;
            let candidate = entry.path().join("bin").join(GUEST_INIT_BASENAME);
            if is_executable_file(&candidate) {
                matches.push(candidate);
            }
        }
    }

    match matches.len() {
        1 => Ok(matches.remove(0)),
        0 => bail!(
            "microvm image is not agentbox-compatible: no executable {GUEST_INIT_BASENAME} found under {}/nix/store/*/bin",
            task_rootfs.display()
        ),
        count => bail!(
            "microvm image is ambiguous: found {count} executable {GUEST_INIT_BASENAME} binaries under {}/nix/store/*/bin",
            task_rootfs.display()
        ),
    }
}

fn copy_guest_init_override(override_path: &Path, target: &Path) -> Result<()> {
    let source = override_path.canonicalize().with_context(|| {
        format!(
            "failed to resolve microvm guest-init override '{}'",
            override_path.display()
        )
    })?;
    if !is_executable_file(&source) {
        bail!(
            "microvm guest-init override '{}' is not an executable regular file",
            source.display()
        );
    }
    fs::copy(&source, target).with_context(|| {
        format!(
            "failed to copy microvm guest-init override '{}' to '{}'",
            source.display(),
            target.display()
        )
    })?;
    let mode = source
        .metadata()
        .with_context(|| format!("failed to stat '{}'", source.display()))?
        .permissions()
        .mode();
    fs::set_permissions(target, fs::Permissions::from_mode(mode)).with_context(|| {
        format!(
            "failed to preserve executable mode on microvm guest-init target '{}'",
            target.display()
        )
    })
}

fn guest_path(task_rootfs: &Path, host_path: &Path) -> Result<String> {
    let relative = host_path.strip_prefix(task_rootfs).with_context(|| {
        format!(
            "microvm guest-init target '{}' is outside task rootfs '{}'",
            host_path.display(),
            task_rootfs.display()
        )
    })?;
    Ok(format!("/{}", relative.display()))
}

fn is_executable_file(path: &Path) -> bool {
    path.metadata()
        .map(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_guest_init(root: &Path, store_name: &str) -> PathBuf {
        let path = root
            .join("nix/store")
            .join(store_name)
            .join("bin")
            .join(GUEST_INIT_BASENAME);
        fs::create_dir_all(path.parent().unwrap()).expect("parent should be created");
        fs::write(&path, "#!/bin/sh\n").expect("guest init should be written");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755))
            .expect("guest init should be executable");
        path
    }

    #[test]
    fn resolves_exactly_one_executable_guest_init_from_task_rootfs() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let host_path = write_guest_init(temp.path(), "hash-agentbox");

        let resolved = resolve_guest_init(temp.path(), None).expect("guest init should resolve");

        assert_eq!(resolved.host_path, host_path);
        assert_eq!(
            resolved.guest_exec_path,
            "/nix/store/hash-agentbox/bin/agentbox-guest-init"
        );
    }

    #[test]
    fn rejects_zero_or_multiple_guest_init_matches() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        assert!(resolve_guest_init(temp.path(), None).is_err());

        write_guest_init(temp.path(), "hash-one");
        write_guest_init(temp.path(), "hash-two");
        let err = resolve_guest_init(temp.path(), None).expect_err("multiple matches fail");
        assert!(err.to_string().contains("ambiguous"));
    }

    #[test]
    fn override_is_copied_into_task_rootfs_target_and_preserves_mode() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let target = write_guest_init(temp.path(), "hash-agentbox");
        let override_path = temp.path().join("override-agentbox-guest-init");
        fs::write(&override_path, "#!/bin/sh\necho override\n")
            .expect("override should be written");
        fs::set_permissions(&override_path, fs::Permissions::from_mode(0o751))
            .expect("override mode should be set");

        let resolved =
            resolve_guest_init(temp.path(), Some(&override_path)).expect("override should resolve");

        assert_eq!(resolved.host_path, target);
        assert_eq!(
            resolved.guest_exec_path,
            "/nix/store/hash-agentbox/bin/agentbox-guest-init"
        );
        assert_eq!(
            fs::read_to_string(&resolved.host_path).expect("target should be readable"),
            "#!/bin/sh\necho override\n"
        );
        assert_eq!(
            fs::metadata(&resolved.host_path)
                .expect("target metadata should be readable")
                .permissions()
                .mode()
                & 0o777,
            0o751
        );
    }
}
