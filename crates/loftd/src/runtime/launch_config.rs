use anyhow::{Context, Result, anyhow};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

pub(crate) const WORKSPACE_TAG: &str = "loftd-workspace";
pub(crate) const WORKSPACE_TARGET: &str = "/workspace";
const HOST_UID_ENV: &str = "LOFTD_HOST_UID";
const HOST_GID_ENV: &str = "LOFTD_HOST_GID";
const WORKSPACE_TAG_ENV: &str = "LOFTD_WORKSPACE_TAG";
const WORKSPACE_TARGET_ENV: &str = "LOFTD_WORKSPACE_TARGET";
const ENTER_AS_ROOT_ENV: &str = "LOFTD_ENTER_AS_ROOT";
const GUEST_PROFILE_ENV: &str = "LOFTD_GUEST_PROFILE";
const GUEST_DEBUG_ENV: &str = "LOFTD_GUEST_DEBUG";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WorkspaceMount {
    pub(crate) source: PathBuf,
    pub(crate) tag: String,
    pub(crate) target: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DiskAttachment {
    pub(crate) id: String,
    pub(crate) path: PathBuf,
    pub(crate) read_only: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct LaunchSpec<'a> {
    pub(crate) task_rootfs: &'a Path,
    pub(crate) workspace_source: &'a Path,
    pub(crate) guest_init_exec: &'a str,
    pub(crate) mem_gib: Option<u32>,
    pub(crate) debug: bool,
    pub(crate) profile: bool,
    pub(crate) root: bool,
    pub(crate) host_uid: u32,
    pub(crate) host_gid: u32,
    pub(crate) vcpus: u8,
    pub(crate) disks: Vec<DiskAttachment>,
    pub(crate) extra_env: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LaunchConfig {
    pub(crate) task_rootfs: PathBuf,
    pub(crate) workspace: WorkspaceMount,
    pub(crate) disks: Vec<DiskAttachment>,
    pub(crate) ram_mib: u32,
    pub(crate) vcpus: u8,
    pub(crate) workdir: String,
    pub(crate) exec_path: String,
    pub(crate) argv: Vec<String>,
    pub(crate) env: Vec<(String, String)>,
}

impl LaunchConfig {
    pub(crate) fn build_for_task(spec: LaunchSpec<'_>) -> Result<Self> {
        let ram_mib = resolve_ram_mib(spec.mem_gib)?;
        let mut env = vec![
            (HOST_UID_ENV.to_owned(), spec.host_uid.to_string()),
            (HOST_GID_ENV.to_owned(), spec.host_gid.to_string()),
            (WORKSPACE_TAG_ENV.to_owned(), WORKSPACE_TAG.to_owned()),
            (WORKSPACE_TARGET_ENV.to_owned(), WORKSPACE_TARGET.to_owned()),
        ];
        if spec.root {
            env.push((ENTER_AS_ROOT_ENV.to_owned(), "1".to_owned()));
        }
        if spec.profile {
            env.push((GUEST_PROFILE_ENV.to_owned(), "1".to_owned()));
        }
        if spec.debug {
            env.push((GUEST_DEBUG_ENV.to_owned(), "1".to_owned()));
        }
        env.extend(spec.extra_env);

        Ok(Self {
            task_rootfs: spec.task_rootfs.to_path_buf(),
            workspace: WorkspaceMount {
                source: spec.workspace_source.to_path_buf(),
                tag: WORKSPACE_TAG.to_owned(),
                target: WORKSPACE_TARGET.to_owned(),
            },
            disks: spec.disks,
            ram_mib,
            vcpus: spec.vcpus,
            workdir: WORKSPACE_TARGET.to_owned(),
            exec_path: spec.guest_init_exec.to_owned(),
            argv: vec![
                "loftd-guest-init".to_owned(),
                "enter".to_owned(),
                "--".to_owned(),
                "fish".to_owned(),
                "-l".to_owned(),
            ],
            env,
        })
    }

    #[cfg(test)]
    pub(crate) fn env_contains(&self, name: &str, value: &str) -> bool {
        self.env
            .iter()
            .any(|(key, actual)| key == name && actual == value)
    }

    pub(crate) fn write_to(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!(
                    "failed to create loftd launch config dir '{}'",
                    parent.display()
                )
            })?;
        }
        fs::write(path, self.serialize())
            .with_context(|| format!("failed to write loftd launch config '{}'", path.display()))
    }

    pub(crate) fn read_from(path: &Path) -> Result<Self> {
        let text = fs::read_to_string(path)
            .with_context(|| format!("failed to read loftd launch config '{}'", path.display()))?;
        Self::parse(&text)
    }

    fn serialize(&self) -> String {
        let mut out = String::new();
        push_field(
            &mut out,
            "task_rootfs",
            &self.task_rootfs.display().to_string(),
        );
        push_field(
            &mut out,
            "workspace_source",
            &self.workspace.source.display().to_string(),
        );
        push_field(&mut out, "workspace_tag", &self.workspace.tag);
        push_field(&mut out, "workspace_target", &self.workspace.target);
        push_field(&mut out, "ram_mib", &self.ram_mib.to_string());
        push_field(&mut out, "vcpus", &self.vcpus.to_string());
        push_field(&mut out, "workdir", &self.workdir);
        push_field(&mut out, "exec_path", &self.exec_path);
        for (index, disk) in self.disks.iter().enumerate() {
            push_field(&mut out, &format!("disk.{index}.id"), &disk.id);
            push_field(
                &mut out,
                &format!("disk.{index}.path"),
                &disk.path.display().to_string(),
            );
            push_field(
                &mut out,
                &format!("disk.{index}.read_only"),
                if disk.read_only { "true" } else { "false" },
            );
        }
        for (index, arg) in self.argv.iter().enumerate() {
            push_field(&mut out, &format!("argv.{index}"), arg);
        }
        for (index, (key, value)) in self.env.iter().enumerate() {
            push_field(&mut out, &format!("env.{index}"), &format!("{key}={value}"));
        }
        out
    }

    fn parse(text: &str) -> Result<Self> {
        let mut fields = BTreeMap::new();
        let mut argv = BTreeMap::new();
        let mut env = BTreeMap::new();
        let mut disks: BTreeMap<usize, PartialDiskAttachment> = BTreeMap::new();

        for (line_index, line) in text.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let (key, encoded) = line.split_once('=').ok_or_else(|| {
                anyhow!("loftd launch config line {} is missing '='", line_index + 1)
            })?;
            let value = decode_hex(encoded).with_context(|| {
                format!(
                    "loftd launch config line {} has invalid hex",
                    line_index + 1
                )
            })?;
            if let Some(rest) = key.strip_prefix("disk.") {
                let (index, field) = rest.split_once('.').ok_or_else(|| {
                    anyhow!("loftd launch config disk entry {key} is missing field")
                })?;
                let disk = disks.entry(parse_index(key, index)?).or_default();
                match field {
                    "id" => disk.id = Some(value),
                    "path" => disk.path = Some(PathBuf::from(value)),
                    "read_only" => {
                        disk.read_only = Some(match value.as_str() {
                            "true" => true,
                            "false" => false,
                            _ => anyhow::bail!(
                                "loftd launch config disk entry {key} has invalid read_only"
                            ),
                        });
                    }
                    _ => anyhow::bail!("loftd launch config contains unknown key {key}"),
                }
            } else if let Some(index) = key.strip_prefix("argv.") {
                argv.insert(parse_index(key, index)?, value);
            } else if let Some(index) = key.strip_prefix("env.") {
                let (name, actual) = value
                    .split_once('=')
                    .ok_or_else(|| anyhow!("loftd launch config env entry {key} is missing '='"))?;
                env.insert(
                    parse_index(key, index)?,
                    (name.to_owned(), actual.to_owned()),
                );
            } else if matches!(
                key,
                "task_rootfs"
                    | "workspace_source"
                    | "workspace_tag"
                    | "workspace_target"
                    | "ram_mib"
                    | "vcpus"
                    | "workdir"
                    | "exec_path"
            ) {
                if fields.insert(key.to_owned(), value).is_some() {
                    anyhow::bail!("loftd launch config repeats key {key}");
                }
            } else {
                anyhow::bail!("loftd launch config contains unknown key {key}");
            }
        }

        let required = |key: &str| -> Result<String> {
            fields
                .get(key)
                .cloned()
                .ok_or_else(|| anyhow!("loftd launch config missing required key {key}"))
        };
        let ram_mib = required("ram_mib")?
            .parse::<u32>()
            .context("loftd launch config ram_mib is invalid")?;
        let vcpus = required("vcpus")?
            .parse::<u8>()
            .context("loftd launch config vcpus is invalid")?;
        Ok(Self {
            task_rootfs: PathBuf::from(required("task_rootfs")?),
            workspace: WorkspaceMount {
                source: PathBuf::from(required("workspace_source")?),
                tag: required("workspace_tag")?,
                target: required("workspace_target")?,
            },
            disks: disks
                .into_iter()
                .map(|(index, disk)| disk.finish(index))
                .collect::<Result<Vec<_>>>()?,
            ram_mib,
            vcpus,
            workdir: required("workdir")?,
            exec_path: required("exec_path")?,
            argv: argv.into_values().collect(),
            env: env.into_values().collect(),
        })
    }
}

#[derive(Default)]
struct PartialDiskAttachment {
    id: Option<String>,
    path: Option<PathBuf>,
    read_only: Option<bool>,
}

impl PartialDiskAttachment {
    fn finish(self, index: usize) -> Result<DiskAttachment> {
        Ok(DiskAttachment {
            id: self
                .id
                .ok_or_else(|| anyhow!("loftd launch config disk.{index} missing id"))?,
            path: self
                .path
                .ok_or_else(|| anyhow!("loftd launch config disk.{index} missing path"))?,
            read_only: self
                .read_only
                .ok_or_else(|| anyhow!("loftd launch config disk.{index} missing read_only"))?,
        })
    }
}

pub(crate) fn resolve_cpu_count() -> Result<u8> {
    let available = std::thread::available_parallelism()
        .context("failed to detect available CPUs for loftd default")?
        .get();
    let count = if available <= 6 {
        available
    } else {
        available - 2
    };
    u8::try_from(count).context("host available CPU count is too large for libkrun vcpu config")
}

fn resolve_ram_mib(mem_gib: Option<u32>) -> Result<u32> {
    let gib = mem_gib.unwrap_or(4);
    if gib == 0 {
        anyhow::bail!("loftd --mem must be at least 1 GiB");
    }
    gib.checked_mul(1024)
        .ok_or_else(|| anyhow!("loftd --mem is too large for libkrun ram_mib"))
}

fn push_field(out: &mut String, key: &str, value: &str) {
    out.push_str(key);
    out.push('=');
    out.push_str(&encode_hex(value));
    out.push('\n');
}

fn encode_hex(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len() * 2);
    for byte in value.as_bytes() {
        encoded.push_str(&format!("{byte:02x}"));
    }
    encoded
}

fn decode_hex(encoded: &str) -> Result<String> {
    if !encoded.len().is_multiple_of(2) {
        anyhow::bail!("odd number of hex characters");
    }
    let bytes = encoded
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let text = std::str::from_utf8(pair)?;
            Ok(u8::from_str_radix(text, 16)?)
        })
        .collect::<std::result::Result<Vec<_>, anyhow::Error>>()?;
    String::from_utf8(bytes).context("hex decoded value is not UTF-8")
}

fn parse_index(key: &str, value: &str) -> Result<usize> {
    value
        .parse::<usize>()
        .with_context(|| format!("loftd launch config index in {key} is invalid"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn launch_config_defaults_to_guest_init_enter_fish_shell() {
        let config = LaunchConfig::build_for_task(LaunchSpec {
            task_rootfs: Path::new("/state/task/rootfs"),
            workspace_source: Path::new("/workspace-src"),
            guest_init_exec: "/nix/store/hash-loftd/bin/loftd-guest-init",
            mem_gib: Some(4),
            debug: true,
            profile: true,
            root: false,
            host_uid: 1000,
            host_gid: 1001,
            vcpus: 2,
            disks: Vec::new(),
            extra_env: Vec::new(),
        })
        .expect("launch config should build");

        assert_eq!(config.task_rootfs, Path::new("/state/task/rootfs"));
        assert_eq!(config.workspace.source, Path::new("/workspace-src"));
        assert_eq!(config.workspace.tag, "loftd-workspace");
        assert_eq!(config.workspace.target, "/workspace");
        assert_eq!(config.ram_mib, 4096);
        assert_eq!(config.vcpus, 2);
        assert_eq!(config.workdir, "/workspace");
        assert_eq!(
            config.exec_path,
            "/nix/store/hash-loftd/bin/loftd-guest-init"
        );
        assert_eq!(
            config.argv,
            ["loftd-guest-init", "enter", "--", "fish", "-l"]
        );
        assert!(config.env_contains("LOFTD_HOST_UID", "1000"));
        assert!(config.env_contains("LOFTD_HOST_GID", "1001"));
        assert!(config.env_contains("LOFTD_WORKSPACE_TAG", "loftd-workspace"));
        assert!(config.env_contains("LOFTD_WORKSPACE_TARGET", "/workspace"));
        assert!(config.env_contains("LOFTD_GUEST_PROFILE", "1"));
        assert!(config.env_contains("LOFTD_GUEST_DEBUG", "1"));
        assert!(
            config
                .env
                .iter()
                .all(|(key, _)| !key.starts_with("AGENTBOX_"))
        );
    }

    #[test]
    fn launch_config_round_trips_through_hex_line_format() {
        let config = LaunchConfig::build_for_task(LaunchSpec {
            task_rootfs: Path::new("/state/task/rootfs"),
            workspace_source: Path::new("/workspace-src"),
            guest_init_exec: "/nix/store/hash-loftd/bin/loftd-guest-init",
            mem_gib: Some(2),
            debug: false,
            profile: false,
            root: true,
            host_uid: 1000,
            host_gid: 1001,
            vcpus: 2,
            disks: vec![
                DiskAttachment {
                    id: "loftd-nix".to_owned(),
                    path: Path::new("/state/loftd-nix.raw").to_path_buf(),
                    read_only: false,
                },
                DiskAttachment {
                    id: "loftd-containers".to_owned(),
                    path: Path::new("/state/loftd-containers.raw").to_path_buf(),
                    read_only: false,
                },
            ],
            extra_env: vec![("LOFTD_CONTAINERS_STORAGE".to_owned(), "1".to_owned())],
        })
        .expect("launch config should build");

        let parsed = LaunchConfig::parse(&config.serialize()).expect("config should parse");

        assert_eq!(parsed, config);
        assert!(parsed.env_contains("LOFTD_ENTER_AS_ROOT", "1"));
        assert!(parsed.env_contains("LOFTD_CONTAINERS_STORAGE", "1"));
        assert_eq!(parsed.disks[0].id, "loftd-nix");
        assert_eq!(
            parsed.disks[1].path,
            Path::new("/state/loftd-containers.raw")
        );
    }

    #[test]
    fn launch_config_rejects_unknown_missing_and_malformed_keys() {
        assert!(LaunchConfig::parse("unknown=61\n").is_err());
        assert!(LaunchConfig::parse("task_rootfs=6\n").is_err());
        assert!(LaunchConfig::parse("task_rootfs=2f\n").is_err());
    }
}
