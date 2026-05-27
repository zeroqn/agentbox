use anyhow::{Context, Result, anyhow};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::CONTAINER_WORKDIR;
use crate::cli::{CommonOptions, MicrovmOptions};

pub(crate) const WORKSPACE_TAG_ENV: &str = "AGENTBOX_MICROVM_WORKSPACE_TAG";
pub(crate) const WORKSPACE_TARGET_ENV: &str = "AGENTBOX_MICROVM_WORKSPACE_TARGET";
const WORKSPACE_TAG: &str = "agentbox-workspace";
const HOST_UID_ENV: &str = "AGENTBOX_HOST_UID";
const HOST_GID_ENV: &str = "AGENTBOX_HOST_GID";
const KVM_DROP_TO_DEV_ENV: &str = "AGENTBOX_KVM_DROP_TO_DEV";
const ENTER_AS_ROOT_ENV: &str = "AGENTBOX_ENTER_AS_ROOT";
const GUEST_PROFILE_ENV: &str = "AGENTBOX_GUEST_PROFILE";
const GUEST_DEBUG_ENV: &str = "AGENTBOX_GUEST_DEBUG";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MicrovmWorkspaceMount {
    pub(crate) source: PathBuf,
    pub(crate) tag: String,
    pub(crate) target: String,
}

#[derive(Debug, Clone)]
pub(crate) struct MicrovmLaunchSpec<'a> {
    pub(crate) task_rootfs: &'a Path,
    pub(crate) workspace_source: &'a Path,
    pub(crate) guest_init_exec: &'a str,
    pub(crate) common: CommonOptions,
    pub(crate) options: MicrovmOptions,
    pub(crate) host_uid: u32,
    pub(crate) host_gid: u32,
    pub(crate) vcpus: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MicrovmLaunchConfig {
    pub(crate) task_rootfs: PathBuf,
    pub(crate) workspace: MicrovmWorkspaceMount,
    pub(crate) ram_mib: u32,
    pub(crate) vcpus: u8,
    pub(crate) workdir: String,
    pub(crate) exec_path: String,
    pub(crate) argv: Vec<String>,
    pub(crate) env: Vec<(String, String)>,
}

impl MicrovmLaunchConfig {
    pub(crate) fn build_for_task(spec: MicrovmLaunchSpec<'_>) -> Result<Self> {
        let ram_mib = resolve_ram_mib(spec.options.mem_gib)?;
        let mut env = vec![
            (HOST_UID_ENV.to_owned(), spec.host_uid.to_string()),
            (HOST_GID_ENV.to_owned(), spec.host_gid.to_string()),
            (KVM_DROP_TO_DEV_ENV.to_owned(), "1".to_owned()),
            (WORKSPACE_TAG_ENV.to_owned(), WORKSPACE_TAG.to_owned()),
            (
                WORKSPACE_TARGET_ENV.to_owned(),
                CONTAINER_WORKDIR.to_owned(),
            ),
        ];
        if spec.common.root {
            env.push((ENTER_AS_ROOT_ENV.to_owned(), "1".to_owned()));
        }
        if spec.common.profile {
            env.push((GUEST_PROFILE_ENV.to_owned(), "1".to_owned()));
        }
        if spec.common.debug {
            env.push((GUEST_DEBUG_ENV.to_owned(), "1".to_owned()));
        }

        Ok(Self {
            task_rootfs: spec.task_rootfs.to_path_buf(),
            workspace: MicrovmWorkspaceMount {
                source: spec.workspace_source.to_path_buf(),
                tag: WORKSPACE_TAG.to_owned(),
                target: CONTAINER_WORKDIR.to_owned(),
            },
            ram_mib,
            vcpus: spec.vcpus,
            workdir: CONTAINER_WORKDIR.to_owned(),
            exec_path: spec.guest_init_exec.to_owned(),
            argv: vec![
                "agentbox-guest-init".to_owned(),
                "microvm".to_owned(),
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
                    "failed to create microvm launch config dir '{}'",
                    parent.display()
                )
            })?;
        }
        fs::write(path, self.serialize())
            .with_context(|| format!("failed to write microvm launch config '{}'", path.display()))
    }

    pub(crate) fn read_from(path: &Path) -> Result<Self> {
        let text = fs::read_to_string(path).with_context(|| {
            format!("failed to read microvm launch config '{}'", path.display())
        })?;
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

        for (line_index, line) in text.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let (key, encoded) = line.split_once('=').ok_or_else(|| {
                anyhow!(
                    "microvm launch config line {} is missing '='",
                    line_index + 1
                )
            })?;
            let value = decode_hex(encoded).with_context(|| {
                format!(
                    "microvm launch config line {} has invalid hex",
                    line_index + 1
                )
            })?;
            if let Some(index) = key.strip_prefix("argv.") {
                argv.insert(parse_index(key, index)?, value);
            } else if let Some(index) = key.strip_prefix("env.") {
                let (name, actual) = value.split_once('=').ok_or_else(|| {
                    anyhow!("microvm launch config env entry {key} is missing '='")
                })?;
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
                    anyhow::bail!("microvm launch config repeats key {key}");
                }
            } else {
                anyhow::bail!("microvm launch config contains unknown key {key}");
            }
        }

        let required = |key: &str| -> Result<String> {
            fields
                .get(key)
                .cloned()
                .ok_or_else(|| anyhow!("microvm launch config missing required key {key}"))
        };
        let ram_mib = required("ram_mib")?
            .parse::<u32>()
            .context("microvm launch config ram_mib is invalid")?;
        let vcpus = required("vcpus")?
            .parse::<u8>()
            .context("microvm launch config vcpus is invalid")?;
        Ok(Self {
            task_rootfs: PathBuf::from(required("task_rootfs")?),
            workspace: MicrovmWorkspaceMount {
                source: PathBuf::from(required("workspace_source")?),
                tag: required("workspace_tag")?,
                target: required("workspace_target")?,
            },
            ram_mib,
            vcpus,
            workdir: required("workdir")?,
            exec_path: required("exec_path")?,
            argv: argv.into_values().collect(),
            env: env.into_values().collect(),
        })
    }
}

pub(crate) fn resolve_cpu_count() -> Result<u8> {
    let available = std::thread::available_parallelism()
        .context("failed to detect available CPUs for microvm default")?
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
        anyhow::bail!("microvm --mem must be at least 1 GiB");
    }
    gib.checked_mul(1024)
        .ok_or_else(|| anyhow!("microvm --mem is too large for libkrun ram_mib"))
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
        .with_context(|| format!("microvm launch config index in {key} is invalid"))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::cli::{CommonOptions, MicrovmOptions, MicrovmStoragePolicy};
    use crate::runtime::microvm::launch::{MicrovmLaunchConfig, MicrovmLaunchSpec};

    #[test]
    fn launch_config_defaults_to_guest_init_microvm_enter_fish_shell() {
        let config = MicrovmLaunchConfig::build_for_task(MicrovmLaunchSpec {
            task_rootfs: Path::new("/state/task/rootfs"),
            workspace_source: Path::new("/workspace-src"),
            guest_init_exec: "/nix/store/hash-agentbox/bin/agentbox-guest-init",
            common: CommonOptions {
                image: None,
                pull_latest: false,
                debug: true,
                profile: true,
                root: false,
            },
            options: MicrovmOptions {
                storage: MicrovmStoragePolicy::Auto,
                guest_init: None,
                preserve_debug: false,
                mem_gib: Some(4),
            },
            host_uid: 1000,
            host_gid: 1001,
            vcpus: 2,
        })
        .expect("launch config should build");

        assert_eq!(config.task_rootfs, Path::new("/state/task/rootfs"));
        assert_eq!(config.workspace.source, Path::new("/workspace-src"));
        assert_eq!(config.workspace.tag, "agentbox-workspace");
        assert_eq!(config.workspace.target, "/workspace");
        assert_eq!(config.ram_mib, 4096);
        assert_eq!(config.vcpus, 2);
        assert_eq!(config.workdir, "/workspace");
        assert_eq!(
            config.exec_path,
            "/nix/store/hash-agentbox/bin/agentbox-guest-init"
        );
        assert_eq!(
            config.argv,
            [
                "agentbox-guest-init",
                "microvm",
                "enter",
                "--",
                "fish",
                "-l"
            ]
        );
        assert!(config.env_contains("AGENTBOX_HOST_UID", "1000"));
        assert!(config.env_contains("AGENTBOX_HOST_GID", "1001"));
        assert!(config.env_contains("AGENTBOX_MICROVM_WORKSPACE_TAG", "agentbox-workspace"));
        assert!(config.env_contains("AGENTBOX_MICROVM_WORKSPACE_TARGET", "/workspace"));
        assert!(config.env_contains("AGENTBOX_GUEST_PROFILE", "1"));
        assert!(config.env_contains("AGENTBOX_GUEST_DEBUG", "1"));
        assert!(!config.argv.iter().any(|arg| arg.contains("Entrypoint")));
        assert!(!config.argv.iter().any(|arg| arg.contains("Cmd")));
    }

    #[test]
    fn launch_config_round_trips_through_hex_line_format() {
        let config = MicrovmLaunchConfig::build_for_task(MicrovmLaunchSpec {
            task_rootfs: Path::new("/state/task/rootfs"),
            workspace_source: Path::new("/workspace-src"),
            guest_init_exec: "/nix/store/hash-agentbox/bin/agentbox-guest-init",
            common: CommonOptions {
                image: None,
                pull_latest: false,
                debug: false,
                profile: false,
                root: true,
            },
            options: MicrovmOptions {
                storage: MicrovmStoragePolicy::Auto,
                guest_init: None,
                preserve_debug: false,
                mem_gib: Some(2),
            },
            host_uid: 1000,
            host_gid: 1001,
            vcpus: 2,
        })
        .expect("launch config should build");

        let parsed = MicrovmLaunchConfig::parse(&config.serialize()).expect("config should parse");

        assert_eq!(parsed, config);
        assert!(parsed.env_contains("AGENTBOX_ENTER_AS_ROOT", "1"));
    }

    #[test]
    fn launch_config_rejects_unknown_missing_and_malformed_keys() {
        assert!(MicrovmLaunchConfig::parse("unknown=61\n").is_err());
        assert!(MicrovmLaunchConfig::parse("task_rootfs=6\n").is_err());
        assert!(MicrovmLaunchConfig::parse("task_rootfs=2f\n").is_err());
    }
}
