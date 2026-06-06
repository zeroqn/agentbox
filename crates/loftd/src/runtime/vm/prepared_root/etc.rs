use anyhow::{Context, Result, bail};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

const HOST_RESOLV_CONF: &str = "/etc/resolv.conf";
const SYSTEMD_RESOLVED_RESOLV_CONF: &str = "/run/systemd/resolve/resolv.conf";
const NETWORK_MANAGER_RESOLV_CONF: &str = "/run/NetworkManager/no-stub-resolv.conf";
const RUNTIME_FILE_MODE: u32 = 0o644;
pub(crate) const HOST_GATEWAY_ADDR: &str = "169.254.1.2";
const HOST_CONTAINERS_INTERNAL: &str = "host.containers.internal";
const HOST_DOCKER_INTERNAL: &str = "host.docker.internal";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RuntimeEtcFiles {
    pub(crate) hostname: String,
    pub(crate) hosts: String,
    pub(crate) resolv_conf: String,
}

pub(crate) fn build(hostname: &str) -> Result<RuntimeEtcFiles> {
    Ok(RuntimeEtcFiles {
        hostname: hostname_file(hostname),
        hosts: hosts_file(hostname),
        resolv_conf: read_runtime_resolv_conf(&ResolverPaths::host_defaults())?,
    })
}

pub(crate) fn materialize(root_export: &Path, files: &RuntimeEtcFiles) -> Result<()> {
    let etc_dir = require_real_etc_dir(root_export)?;
    let entries = [
        ("hostname", files.hostname.as_str()),
        ("hosts", files.hosts.as_str()),
        ("resolv.conf", files.resolv_conf.as_str()),
    ];
    for (name, _) in entries {
        validate_replaceable_runtime_file(&etc_dir.join(name))?;
    }
    for (name, content) in entries {
        replace_regular_file_no_follow(&etc_dir.join(name), content)?;
    }
    Ok(())
}

pub(crate) fn require_real_etc_dir(root_export: &Path) -> Result<PathBuf> {
    let etc_dir = root_export.join("etc");
    let metadata = fs::symlink_metadata(&etc_dir).with_context(|| {
        format!(
            "failed to inspect prepared-root runtime etc dir '{}'",
            etc_dir.display()
        )
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        bail!(
            "loftd prepared root requires '{}' to be a real directory",
            etc_dir.display()
        );
    }
    Ok(etc_dir)
}

fn hostname_file(hostname: &str) -> String {
    format!("{hostname}\n")
}

fn hosts_file(hostname: &str) -> String {
    format!(
        "127.0.0.1\tlocalhost {hostname}\n::1\tlocalhost ip6-localhost ip6-loopback\n{HOST_GATEWAY_ADDR}\t{HOST_CONTAINERS_INTERNAL} {HOST_DOCKER_INTERNAL}\n"
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ResolverPaths {
    host: PathBuf,
    systemd_resolved: PathBuf,
    network_manager: PathBuf,
}

impl ResolverPaths {
    fn host_defaults() -> Self {
        Self {
            host: PathBuf::from(HOST_RESOLV_CONF),
            systemd_resolved: PathBuf::from(SYSTEMD_RESOLVED_RESOLV_CONF),
            network_manager: PathBuf::from(NETWORK_MANAGER_RESOLV_CONF),
        }
    }
}

fn read_runtime_resolv_conf(paths: &ResolverPaths) -> Result<String> {
    let host = fs::read_to_string(&paths.host)
        .with_context(|| format!("failed to read host resolver '{}'", paths.host.display()))?;
    if has_nameserver(&host, "127.0.0.53") {
        return Ok(read_stub_target_or_original(&paths.systemd_resolved, &host));
    }
    if has_nameserver(&host, "127.0.0.1") {
        return Ok(read_stub_target_or_original(&paths.network_manager, &host));
    }
    Ok(host)
}

fn read_stub_target_or_original(stub_target: &Path, original: &str) -> String {
    // Match Podman's fail-safe resolver behavior: prefer the real resolver behind
    // a localhost stub, but keep the host file if the stub target is unavailable.
    fs::read_to_string(stub_target).unwrap_or_else(|_| original.to_owned())
}

fn has_nameserver(resolv_conf: &str, address: &str) -> bool {
    resolv_conf.lines().any(|line| {
        let mut parts = line.split_whitespace();
        parts.next() == Some("nameserver") && parts.next() == Some(address)
    })
}

fn validate_replaceable_runtime_file(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || metadata.is_file() => Ok(()),
        Ok(_) => bail!(
            "loftd runtime etc path '{}' must be a regular file or symlink",
            path.display()
        ),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err)
            .with_context(|| format!("failed to inspect runtime etc path '{}'", path.display())),
    }
}

fn replace_regular_file_no_follow(path: &Path, content: &str) -> Result<()> {
    if fs::symlink_metadata(path).is_ok() {
        fs::remove_file(path)
            .with_context(|| format!("failed to remove runtime etc path '{}'", path.display()))?;
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .with_context(|| format!("failed to create runtime etc file '{}'", path.display()))?;
    file.write_all(content.as_bytes())
        .with_context(|| format!("failed to write runtime etc file '{}'", path.display()))?;
    file.set_permissions(fs::Permissions::from_mode(RUNTIME_FILE_MODE))
        .with_context(|| format!("failed to chmod runtime etc file '{}'", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{MetadataExt, symlink};

    fn files() -> RuntimeEtcFiles {
        RuntimeEtcFiles {
            hostname: "loftd-workspace\n".to_owned(),
            hosts: "127.0.0.1\tlocalhost loftd-workspace\n::1\tlocalhost ip6-localhost ip6-loopback\n169.254.1.2\thost.containers.internal host.docker.internal\n"
                .to_owned(),
            resolv_conf: "nameserver 192.0.2.53\nsearch example.test\n".to_owned(),
        }
    }

    #[test]
    fn hosts_file_always_contains_podman_like_host_aliases() {
        let hosts = hosts_file("loftd-workspace");

        assert!(hosts.contains("127.0.0.1\tlocalhost loftd-workspace\n"));
        assert!(hosts.contains("169.254.1.2\thost.containers.internal host.docker.internal\n"));
    }

    #[test]
    fn materialize_creates_regular_runtime_etc_files_when_etc_exists() {
        let dir = tempfile::tempdir().expect("tempdir");
        fs::create_dir(dir.path().join("etc")).expect("etc");

        materialize(dir.path(), &files()).expect("runtime etc should materialize");

        for name in ["hostname", "hosts", "resolv.conf"] {
            let metadata = fs::symlink_metadata(dir.path().join("etc").join(name)).expect(name);
            assert!(metadata.is_file());
            assert!(!metadata.file_type().is_symlink());
            assert_eq!(metadata.mode() & 0o777, RUNTIME_FILE_MODE);
        }
        assert_eq!(
            fs::read_to_string(dir.path().join("etc/hostname")).expect("hostname"),
            "loftd-workspace\n"
        );
    }

    #[test]
    fn materialize_overwrites_existing_runtime_files() {
        let dir = tempfile::tempdir().expect("tempdir");
        let etc = dir.path().join("etc");
        fs::create_dir(&etc).expect("etc");
        for name in ["hostname", "hosts", "resolv.conf"] {
            fs::write(etc.join(name), "image value\n").expect("seed image file");
        }

        materialize(dir.path(), &files()).expect("runtime etc should overwrite");

        assert_eq!(
            fs::read_to_string(etc.join("hosts")).expect("hosts"),
            files().hosts
        );
        assert_eq!(
            fs::read_to_string(etc.join("resolv.conf")).expect("resolv"),
            files().resolv_conf
        );
    }

    #[test]
    fn materialize_replaces_final_symlink_without_touching_target() {
        let dir = tempfile::tempdir().expect("tempdir");
        let etc = dir.path().join("etc");
        fs::create_dir(&etc).expect("etc");
        let sentinel = dir.path().join("sentinel");
        fs::write(&sentinel, "sentinel\n").expect("sentinel");
        symlink(&sentinel, etc.join("resolv.conf")).expect("symlink");

        materialize(dir.path(), &files()).expect("runtime etc should materialize");

        assert_eq!(
            fs::read_to_string(&sentinel).expect("sentinel"),
            "sentinel\n"
        );
        let resolv = etc.join("resolv.conf");
        let metadata = fs::symlink_metadata(&resolv).expect("resolv");
        assert!(metadata.is_file());
        assert!(!metadata.file_type().is_symlink());
        assert_eq!(
            fs::read_to_string(resolv).expect("resolv"),
            files().resolv_conf
        );
    }

    #[test]
    fn materialize_rejects_symlinked_etc_parent_before_touching_files() {
        let dir = tempfile::tempdir().expect("tempdir");
        let real_etc = dir.path().join("real-etc");
        fs::create_dir(&real_etc).expect("real etc");
        symlink(&real_etc, dir.path().join("etc")).expect("etc symlink");

        let err = materialize(dir.path(), &files()).expect_err("symlinked etc should fail");

        assert!(format!("{err:#}").contains("real directory"));
        assert!(!real_etc.join("hostname").exists());
        assert!(!real_etc.join("hosts").exists());
        assert!(!real_etc.join("resolv.conf").exists());
    }

    #[test]
    fn materialize_rejects_missing_etc_before_touching_files() {
        let dir = tempfile::tempdir().expect("tempdir");

        let err = materialize(dir.path(), &files()).expect_err("missing etc should fail");

        assert!(format!("{err:#}").contains("failed to inspect"));
        assert!(!dir.path().join("hostname").exists());
    }

    #[test]
    fn materialize_rejects_directory_runtime_file_before_touching_files() {
        let dir = tempfile::tempdir().expect("tempdir");
        let etc = dir.path().join("etc");
        fs::create_dir(&etc).expect("etc");
        fs::create_dir(etc.join("resolv.conf")).expect("directory resolv");

        let err = materialize(dir.path(), &files()).expect_err("directory file should fail");

        assert!(format!("{err:#}").contains("regular file or symlink"));
        assert!(!etc.join("hostname").exists());
        assert!(!etc.join("hosts").exists());
    }

    #[test]
    fn resolver_uses_host_resolv_conf_without_stub_nameserver() {
        let dir = tempfile::tempdir().expect("tempdir");
        let host = dir.path().join("resolv.conf");
        fs::write(&host, "search example.test\nnameserver 192.0.2.53\n").expect("host");
        let paths = ResolverPaths {
            host,
            systemd_resolved: dir.path().join("systemd-resolv.conf"),
            network_manager: dir.path().join("nm-resolv.conf"),
        };

        assert_eq!(
            read_runtime_resolv_conf(&paths).expect("resolv"),
            "search example.test\nnameserver 192.0.2.53\n"
        );
    }

    #[test]
    fn resolver_follows_systemd_resolved_stub_when_available() {
        let dir = tempfile::tempdir().expect("tempdir");
        let host = dir.path().join("resolv.conf");
        let systemd = dir.path().join("systemd-resolv.conf");
        fs::write(&host, "nameserver 127.0.0.53\noptions edns0\n").expect("host");
        fs::write(&systemd, "nameserver 192.0.2.53\nsearch resolved.test\n").expect("systemd");
        let paths = ResolverPaths {
            host,
            systemd_resolved: systemd,
            network_manager: dir.path().join("nm-resolv.conf"),
        };

        assert_eq!(
            read_runtime_resolv_conf(&paths).expect("resolv"),
            "nameserver 192.0.2.53\nsearch resolved.test\n"
        );
    }

    #[test]
    fn resolver_follows_network_manager_stub_when_available() {
        let dir = tempfile::tempdir().expect("tempdir");
        let host = dir.path().join("resolv.conf");
        let nm = dir.path().join("nm-resolv.conf");
        fs::write(&host, "nameserver 127.0.0.1\n").expect("host");
        fs::write(&nm, "nameserver 192.0.2.54\nsearch nm.test\n").expect("nm");
        let paths = ResolverPaths {
            host,
            systemd_resolved: dir.path().join("systemd-resolv.conf"),
            network_manager: nm,
        };

        assert_eq!(
            read_runtime_resolv_conf(&paths).expect("resolv"),
            "nameserver 192.0.2.54\nsearch nm.test\n"
        );
    }

    #[test]
    fn resolver_keeps_original_stub_when_fallback_is_missing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let host = dir.path().join("resolv.conf");
        fs::write(&host, "nameserver 127.0.0.53\n").expect("host");
        let paths = ResolverPaths {
            host,
            systemd_resolved: dir.path().join("missing-systemd-resolv.conf"),
            network_manager: dir.path().join("missing-nm-resolv.conf"),
        };

        assert_eq!(
            read_runtime_resolv_conf(&paths).expect("resolv"),
            "nameserver 127.0.0.53\n"
        );
    }
}
