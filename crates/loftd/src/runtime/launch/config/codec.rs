//! Hex-line launch-config serialization contract.

use anyhow::{Context, Result, anyhow};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::logging::LogLevel;

use super::components::{guest_init, mounts};
use super::guest_env::guest_config_json;
use super::model::{
    BindMount, DiskAttachment, GuestInitOverrideMount, LOFTD_KRUN_CONFIG_PATH, LaunchConfig,
    NetworkMode,
};

impl LaunchConfig {
    pub(crate) fn write_guest_config_to_rootfs(&self) -> Result<PathBuf> {
        let relative_path = LOFTD_KRUN_CONFIG_PATH
            .strip_prefix('/')
            .ok_or_else(|| anyhow!("loftd krun config path must be absolute"))?;
        let path = self.task_rootfs.join(relative_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!(
                    "failed to create loftd krun config dir '{}'",
                    parent.display()
                )
            })?;
        }
        fs::write(&path, guest_config_json(&self.guest_config_env))
            .with_context(|| format!("failed to write loftd krun config '{}'", path.display()))?;
        Ok(path)
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

    pub(crate) fn decode_file_for_debug(path: &Path) -> Result<String> {
        let text = fs::read_to_string(path)
            .with_context(|| format!("failed to read loftd launch config '{}'", path.display()))?;
        decode_text_for_debug(&text)
    }

    pub(crate) fn serialize(&self) -> String {
        let mut out = String::new();
        push_field(
            &mut out,
            "task_rootfs",
            &self.task_rootfs.display().to_string(),
        );
        push_field(&mut out, "hostname", &self.hostname);
        if let Ok(workspace) = mounts::workspace_mount(&self.mounts) {
            push_field(
                &mut out,
                "workspace_source",
                &workspace.source.display().to_string(),
            );
            push_field(&mut out, "workspace_tag", &workspace.tag);
            push_field(&mut out, "workspace_target", &workspace.target);
        }
        for (index, mount) in self.mounts.iter().enumerate() {
            push_field(
                &mut out,
                &format!("mount.{index}.source"),
                &mount.source.display().to_string(),
            );
            push_field(&mut out, &format!("mount.{index}.tag"), &mount.tag);
            push_field(&mut out, &format!("mount.{index}.target"), &mount.target);
        }
        if let Some(mount) = &self.guest_init_override {
            push_field(
                &mut out,
                "guest_init_override_source",
                &mount.source.display().to_string(),
            );
            push_field(&mut out, "guest_init_override_target", &mount.target);
            push_field(
                &mut out,
                "guest_init_override_read_only",
                if mount.read_only { "true" } else { "false" },
            );
        }
        push_field(&mut out, "ram_mib", &self.ram_mib.to_string());
        push_field(&mut out, "vcpus", &self.vcpus.to_string());
        push_field(&mut out, "log_level", self.log_level.as_str());
        push_field(
            &mut out,
            "network_mode",
            self.network_mode.as_config_value(),
        );
        for (index, spec) in self.publish.iter().enumerate() {
            push_field(&mut out, &format!("publish.{index}"), spec);
        }
        push_field(&mut out, "workdir", &self.workdir);
        push_field(&mut out, "exec_path", &self.exec_path);
        if let Some(passt_socket) = &self.passt_socket {
            push_field(
                &mut out,
                "passt_socket",
                &passt_socket.display().to_string(),
            );
        }
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
        for (index, (key, value)) in self.guest_config_env.iter().enumerate() {
            push_field(
                &mut out,
                &format!("guest_env.{index}"),
                &format!("{key}={value}"),
            );
        }
        out
    }

    pub(crate) fn parse(text: &str) -> Result<Self> {
        let mut fields = BTreeMap::new();
        let mut argv = BTreeMap::new();
        let mut env = BTreeMap::new();
        let mut guest_config_env = BTreeMap::new();
        let mut publish = BTreeMap::new();
        let mut mounts: BTreeMap<usize, PartialBindMount> = BTreeMap::new();
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
            if let Some(rest) = key.strip_prefix("mount.") {
                let (index, field) = rest.split_once('.').ok_or_else(|| {
                    anyhow!("loftd launch config mount entry {key} is missing field")
                })?;
                let mount = mounts.entry(parse_index(key, index)?).or_default();
                match field {
                    "source" => mount.source = Some(PathBuf::from(value)),
                    "tag" => mount.tag = Some(value),
                    "target" => mount.target = Some(value),
                    _ => anyhow::bail!("loftd launch config contains unknown key {key}"),
                }
            } else if let Some(rest) = key.strip_prefix("disk.") {
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
            } else if let Some(index) = key.strip_prefix("publish.") {
                publish.insert(parse_index(key, index)?, value);
            } else if let Some(index) = key.strip_prefix("env.") {
                let (name, actual) = value
                    .split_once('=')
                    .ok_or_else(|| anyhow!("loftd launch config env entry {key} is missing '='"))?;
                env.insert(
                    parse_index(key, index)?,
                    (name.to_owned(), actual.to_owned()),
                );
            } else if let Some(index) = key.strip_prefix("guest_env.") {
                let (name, actual) = value.split_once('=').ok_or_else(|| {
                    anyhow!("loftd launch config guest env entry {key} is missing '='")
                })?;
                guest_config_env.insert(
                    parse_index(key, index)?,
                    (name.to_owned(), actual.to_owned()),
                );
            } else if matches!(
                key,
                "task_rootfs"
                    | "workspace_source"
                    | "workspace_tag"
                    | "workspace_target"
                    | "guest_init_override_source"
                    | "guest_init_override_target"
                    | "guest_init_override_read_only"
                    | "hostname"
                    | "ram_mib"
                    | "vcpus"
                    | "log_level"
                    | "network_mode"
                    | "workdir"
                    | "exec_path"
                    | "passt_socket"
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
        let log_level_text = required("log_level")?;
        let log_level = LogLevel::parse_name(&log_level_text)
            .ok_or_else(|| anyhow!("loftd launch config log_level is invalid"))?;
        let network_mode = NetworkMode::parse_config_value(&required("network_mode")?)?;
        let mounts = parse_mounts(&fields, mounts)?;
        let guest_init_override =
            parse_guest_init_override_mount(&fields, required("exec_path")?.as_str())?;
        Ok(Self {
            task_rootfs: PathBuf::from(required("task_rootfs")?),
            hostname: required("hostname")?,
            mounts,
            guest_init_override,
            disks: disks
                .into_iter()
                .map(|(index, disk)| disk.finish(index))
                .collect::<Result<Vec<_>>>()?,
            ram_mib,
            vcpus,
            log_level,
            network_mode,
            publish: publish.into_values().collect(),
            workdir: required("workdir")?,
            exec_path: required("exec_path")?,
            argv: argv.into_values().collect(),
            env: env.into_values().collect(),
            guest_config_env: guest_config_env.into_values().collect(),
            passt_socket: fields.get("passt_socket").map(PathBuf::from),
        })
    }
}

#[derive(Default)]
struct PartialBindMount {
    source: Option<PathBuf>,
    tag: Option<String>,
    target: Option<String>,
}

impl PartialBindMount {
    fn finish(self, index: usize) -> Result<BindMount> {
        Ok(BindMount {
            source: self
                .source
                .ok_or_else(|| anyhow!("loftd launch config mount.{index} missing source"))?,
            tag: self
                .tag
                .ok_or_else(|| anyhow!("loftd launch config mount.{index} missing tag"))?,
            target: self
                .target
                .ok_or_else(|| anyhow!("loftd launch config mount.{index} missing target"))?,
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

fn parse_guest_init_override_mount(
    fields: &BTreeMap<String, String>,
    exec_path: &str,
) -> Result<Option<GuestInitOverrideMount>> {
    let keys_present = [
        "guest_init_override_source",
        "guest_init_override_target",
        "guest_init_override_read_only",
    ]
    .iter()
    .any(|key| fields.contains_key(*key));
    if !keys_present {
        return Ok(None);
    }

    let mount = GuestInitOverrideMount {
        source: PathBuf::from(required_field(fields, "guest_init_override_source")?),
        target: required_field(fields, "guest_init_override_target")?,
        read_only: match required_field(fields, "guest_init_override_read_only")?.as_str() {
            "true" => true,
            "false" => false,
            _ => anyhow::bail!("loftd launch config guest_init_override_read_only is invalid"),
        },
    };
    guest_init::validate_guest_init_override_mount(&mount, exec_path)?;
    Ok(Some(mount))
}

fn parse_mounts(
    fields: &BTreeMap<String, String>,
    indexed_mounts: BTreeMap<usize, PartialBindMount>,
) -> Result<Vec<BindMount>> {
    let has_indexed_mounts = !indexed_mounts.is_empty();
    let mounts = if !has_indexed_mounts {
        vec![BindMount {
            source: PathBuf::from(required_field(fields, "workspace_source")?),
            tag: required_field(fields, "workspace_tag")?,
            target: required_field(fields, "workspace_target")?,
        }]
    } else {
        indexed_mounts
            .into_iter()
            .map(|(index, mount)| mount.finish(index))
            .collect::<Result<Vec<_>>>()?
    };

    if has_indexed_mounts && has_legacy_workspace_fields(fields) {
        let legacy = BindMount {
            source: PathBuf::from(required_field(fields, "workspace_source")?),
            tag: required_field(fields, "workspace_tag")?,
            target: required_field(fields, "workspace_target")?,
        };
        let workspace = mounts::workspace_mount(&mounts)?;
        if *workspace != legacy {
            anyhow::bail!(
                "loftd launch config workspace compatibility fields disagree with indexed mounts"
            );
        }
    }

    mounts::validate_mounts(&mounts)?;
    Ok(mounts)
}

fn has_legacy_workspace_fields(fields: &BTreeMap<String, String>) -> bool {
    fields.contains_key("workspace_source")
        || fields.contains_key("workspace_tag")
        || fields.contains_key("workspace_target")
}

fn required_field(fields: &BTreeMap<String, String>, key: &str) -> Result<String> {
    fields
        .get(key)
        .cloned()
        .ok_or_else(|| anyhow!("loftd launch config missing required key {key}"))
}

pub(crate) fn push_field(out: &mut String, key: &str, value: &str) {
    out.push_str(key);
    out.push('=');
    out.push_str(&encode_hex(value));
    out.push('\n');
}

pub(crate) fn decode_text_for_debug(text: &str) -> Result<String> {
    let mut out = String::new();
    for (line_index, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let (key, encoded) = line
            .split_once('=')
            .ok_or_else(|| anyhow!("loftd launch config line {} is missing '='", line_index + 1))?;
        let value = decode_hex(encoded).with_context(|| {
            format!(
                "loftd launch config line {} has invalid hex",
                line_index + 1
            )
        })?;
        out.push_str(key);
        out.push('=');
        out.push_str(&value.escape_debug().to_string());
        out.push('\n');
    }
    Ok(out)
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
