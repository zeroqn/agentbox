use anyhow::{Context, Result, bail};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use crate::runtime::launch::config::GuestInitOverrideMount;

const GUEST_INIT_BASENAME: &str = "loftd-guest-init";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedGuestInit {
    pub(crate) host_path: PathBuf,
    pub(crate) guest_exec_path: String,
    pub(crate) override_mount: Option<GuestInitOverrideMount>,
}

pub(crate) fn resolve_guest_init_with_entrypoint(
    task_rootfs: &Path,
    override_path: Option<&Path>,
    image_entrypoint: &[String],
) -> Result<ResolvedGuestInit> {
    let target = find_loftd_guest_init(task_rootfs)?;
    let guest_exec_path = guest_path(task_rootfs, &target)?;
    if let Some(override_path) = override_path {
        let source = validate_guest_init_override(override_path)?;
        return Ok(ResolvedGuestInit {
            guest_exec_path: guest_exec_path.clone(),
            host_path: target,
            override_mount: Some(GuestInitOverrideMount {
                source,
                target: guest_exec_path,
                read_only: true,
            }),
        });
    }

    if !image_entrypoint.is_empty() {
        return resolve_guest_init_entrypoint(task_rootfs, image_entrypoint);
    }

    Ok(ResolvedGuestInit {
        guest_exec_path,
        host_path: target,
        override_mount: None,
    })
}

fn resolve_guest_init_entrypoint(
    task_rootfs: &Path,
    image_entrypoint: &[String],
) -> Result<ResolvedGuestInit> {
    match image_entrypoint {
        [exec_path, enter, separator] if enter == "enter" && separator == "--" => {
            let guest_path = Path::new(exec_path);
            if !guest_path.is_absolute() {
                bail!(
                    "loftd image entrypoint '{}' is not absolute; direct-libkrun requires an absolute {GUEST_INIT_BASENAME} entrypoint",
                    exec_path
                );
            }
            if guest_path.file_name().and_then(|name| name.to_str()) != Some(GUEST_INIT_BASENAME) {
                bail!(
                    "loftd image entrypoint '{}' does not point to {GUEST_INIT_BASENAME}",
                    exec_path
                );
            }
            let host_path = task_rootfs.join(
                guest_path
                    .strip_prefix("/")
                    .context("absolute guest-init path should strip root prefix")?,
            );
            if !is_executable_file(&host_path) {
                bail!(
                    "loftd image entrypoint '{}' does not resolve to an executable file in task rootfs '{}'",
                    exec_path,
                    task_rootfs.display()
                );
            }
            Ok(ResolvedGuestInit {
                host_path,
                guest_exec_path: exec_path.to_owned(),
                override_mount: None,
            })
        }
        [_, enter, separator, extra @ ..] if enter == "enter" && separator == "--" => {
            bail!(
                "loftd image entrypoint has unsupported extra args after 'enter --': {:?}",
                extra
            )
        }
        _ => bail!(
            "loftd image entrypoint is not compatible with direct-libkrun; expected [*/{GUEST_INIT_BASENAME}, \"enter\", \"--\"]"
        ),
    }
}

fn find_loftd_guest_init(task_rootfs: &Path) -> Result<PathBuf> {
    let store = task_rootfs.join("nix/store");
    let mut matches = Vec::new();
    if store.is_dir() {
        for entry in fs::read_dir(&store).with_context(|| {
            format!(
                "failed to read loftd task rootfs store '{}'",
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
            "loftd image is not loftd-compatible: no executable {GUEST_INIT_BASENAME} found under {}/nix/store/*/bin",
            task_rootfs.display()
        ),
        count => bail!(
            "loftd image is ambiguous: found {count} executable {GUEST_INIT_BASENAME} binaries under {}/nix/store/*/bin",
            task_rootfs.display()
        ),
    }
}

fn validate_guest_init_override(override_path: &Path) -> Result<PathBuf> {
    let source = override_path.canonicalize().with_context(|| {
        format!(
            "failed to resolve loftd guest-init override '{}'",
            override_path.display()
        )
    })?;
    if !is_executable_file(&source) {
        bail!(
            "loftd guest-init override '{}' is not an executable regular file",
            source.display()
        );
    }
    Ok(source)
}

fn guest_path(task_rootfs: &Path, host_path: &Path) -> Result<String> {
    let relative = host_path.strip_prefix(task_rootfs).with_context(|| {
        format!(
            "loftd guest-init target '{}' is outside task rootfs '{}'",
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
        let host_path = write_guest_init(temp.path(), "hash-loftd");

        let resolved = resolve_guest_init_with_entrypoint(temp.path(), None, &[])
            .expect("guest init should resolve");

        assert_eq!(resolved.host_path, host_path);
        assert_eq!(
            resolved.guest_exec_path,
            "/nix/store/hash-loftd/bin/loftd-guest-init"
        );
        assert_eq!(resolved.override_mount, None);
    }

    #[test]
    fn rejects_zero_or_multiple_guest_init_matches() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        assert!(resolve_guest_init_with_entrypoint(temp.path(), None, &[]).is_err());

        write_guest_init(temp.path(), "hash-one");
        write_guest_init(temp.path(), "hash-two");
        let err = resolve_guest_init_with_entrypoint(temp.path(), None, &[])
            .expect_err("multiple matches fail");
        assert!(err.to_string().contains("ambiguous"));
    }

    #[test]
    fn override_is_bound_read_only_over_task_rootfs_target_without_mutating_target() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let target = write_guest_init(temp.path(), "hash-loftd");
        let override_path = temp.path().join("override-loftd-guest-init");
        fs::write(&override_path, "#!/bin/sh\necho override\n")
            .expect("override should be written");
        fs::set_permissions(&override_path, fs::Permissions::from_mode(0o751))
            .expect("override mode should be set");

        let resolved = resolve_guest_init_with_entrypoint(temp.path(), Some(&override_path), &[])
            .expect("override should resolve");

        assert_eq!(resolved.host_path, target);
        assert_eq!(
            resolved.guest_exec_path,
            "/nix/store/hash-loftd/bin/loftd-guest-init"
        );
        assert_eq!(
            fs::read_to_string(&resolved.host_path).expect("target should be readable"),
            "#!/bin/sh\n"
        );
        assert_eq!(
            fs::metadata(&resolved.host_path)
                .expect("target metadata should be readable")
                .permissions()
                .mode()
                & 0o777,
            0o755
        );
        assert_eq!(
            resolved.override_mount,
            Some(GuestInitOverrideMount {
                source: override_path
                    .canonicalize()
                    .expect("override should canonicalize"),
                target: "/nix/store/hash-loftd/bin/loftd-guest-init".to_owned(),
                read_only: true,
            })
        );
    }

    #[test]
    fn compatible_image_entrypoint_is_used_as_guest_init_exec() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        write_guest_init(temp.path(), "hash-loftd");

        let resolved = resolve_guest_init_with_entrypoint(
            temp.path(),
            None,
            &[
                "/nix/store/hash-loftd/bin/loftd-guest-init".to_owned(),
                "enter".to_owned(),
                "--".to_owned(),
            ],
        )
        .expect("compatible entrypoint should resolve");

        assert_eq!(
            resolved.guest_exec_path,
            "/nix/store/hash-loftd/bin/loftd-guest-init"
        );
    }

    #[test]
    fn incompatible_image_entrypoint_is_rejected() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        write_guest_init(temp.path(), "hash-loftd");

        let cases = [
            vec!["/bin/bash".to_owned()],
            vec![
                "/nix/store/hash-loftd/bin/loftd-guest-init".to_owned(),
                "enter".to_owned(),
                "--".to_owned(),
                "fish".to_owned(),
            ],
            vec![
                "loftd-guest-init".to_owned(),
                "enter".to_owned(),
                "--".to_owned(),
            ],
        ];

        for entrypoint in cases {
            assert!(
                resolve_guest_init_with_entrypoint(temp.path(), None, &entrypoint).is_err(),
                "entrypoint should fail: {entrypoint:?}"
            );
        }
    }

    #[test]
    fn guest_init_override_ignores_image_entrypoint_executable() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let target = write_guest_init(temp.path(), "hash-loftd");
        let override_path = temp.path().join("override-loftd-guest-init");
        fs::write(&override_path, "#!/bin/sh\necho override\n")
            .expect("override should be written");
        fs::set_permissions(&override_path, fs::Permissions::from_mode(0o755))
            .expect("override mode should be set");

        let resolved = resolve_guest_init_with_entrypoint(
            temp.path(),
            Some(&override_path),
            &["/bin/not-loftd".to_owned()],
        )
        .expect("override should win over incompatible image entrypoint");

        assert_eq!(resolved.host_path, target);
        assert_eq!(
            fs::read_to_string(&resolved.host_path).expect("target should be readable"),
            "#!/bin/sh\n"
        );
        assert_eq!(
            resolved.override_mount,
            Some(GuestInitOverrideMount {
                source: override_path
                    .canonicalize()
                    .expect("override should canonicalize"),
                target: "/nix/store/hash-loftd/bin/loftd-guest-init".to_owned(),
                read_only: true,
            })
        );
    }
}
