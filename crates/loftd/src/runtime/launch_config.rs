use anyhow::{Context, Result, anyhow};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::logging::LogLevel;
use crate::runtime::image_source::OciProcessConfig;

pub(crate) const WORKSPACE_TAG: &str = "loftd-workspace";
pub(crate) const WORKSPACE_TARGET: &str = "/workspace";
pub const CODEX_TAG: &str = "loftd-codex";
pub const CODEX_TARGET: &str = "/home/dev/.codex";
pub const PI_TAG: &str = "loftd-pi";
pub const PI_TARGET: &str = "/home/dev/.pi";
pub const CARGO_TAG: &str = "loftd-cargo";
pub const CARGO_TARGET: &str = "/home/dev/.cargo";
pub const SCCACHE_TAG: &str = "loftd-sccache";
pub const SCCACHE_TARGET: &str = "/home/dev/.cache/sccache";
const SCCACHE_DIR_ENV: &str = "SCCACHE_DIR";
const HOST_UID_ENV: &str = "LOFTD_HOST_UID";
const HOST_GID_ENV: &str = "LOFTD_HOST_GID";
const ENTER_AS_ROOT_ENV: &str = "LOFTD_ENTER_AS_ROOT";
const GUEST_PROFILE_ENV: &str = "LOFTD_GUEST_PROFILE";
const GUEST_DEBUG_ENV: &str = "LOFTD_GUEST_DEBUG";
const IMAGE_PATH_ENV: &str = "PATH";
const KRUN_CONFIG_ENV: &str = "KRUN_CONFIG";
pub(crate) const LOFTD_KRUN_CONFIG_PATH: &str = "/.loftd_config.json";
const KIB: u64 = 1024;
const MIB_PER_GIB: u32 = 1024;
const BYTES_PER_GIB: u64 = 1024 * 1024 * 1024;
const MAX_GIB_FOR_KRUN_RAM_MIB: u32 = u32::MAX / MIB_PER_GIB;
const HOST_MEMINFO: &str = "/proc/meminfo";
const IMAGE_LOFTD_ENV_ALLOWLIST: &[&str] = &[
    "LOFTD_FISH_CONFIG_SOURCE",
    "LOFTD_STARSHIP_CONFIG_SOURCE",
    "LOFTD_GRAPHENE_HARDENED_MALLOC_LIB",
    "LOFTD_REAL_PODMAN",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindMount {
    pub source: PathBuf,
    pub tag: String,
    pub target: String,
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
    pub mounts: &'a [BindMount],
    pub(crate) guest_init_exec: &'a str,
    pub(crate) guest_command: &'a [String],
    pub(crate) image_process_config: &'a OciProcessConfig,
    pub(crate) mem_gib: Option<u32>,
    pub(crate) log_level: LogLevel,
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
    pub mounts: Vec<BindMount>,
    pub(crate) disks: Vec<DiskAttachment>,
    pub(crate) ram_mib: u32,
    pub(crate) vcpus: u8,
    pub(crate) log_level: LogLevel,
    pub(crate) workdir: String,
    pub(crate) exec_path: String,
    pub(crate) argv: Vec<String>,
    pub(crate) env: Vec<(String, String)>,
    pub(crate) guest_config_env: Vec<(String, String)>,
}

impl LaunchConfig {
    pub(crate) fn build_for_task(spec: LaunchSpec<'_>) -> Result<Self> {
        let ram_mib = resolve_ram_mib(spec.mem_gib)?;
        validate_mounts(spec.mounts)?;
        let required_env = vec![
            (HOST_UID_ENV.to_owned(), spec.host_uid.to_string()),
            (HOST_GID_ENV.to_owned(), spec.host_gid.to_string()),
            (SCCACHE_DIR_ENV.to_owned(), SCCACHE_TARGET.to_owned()),
        ];
        let mut guest_config_env = bootstrap_env(&spec.image_process_config.env, required_env)?;
        if spec.root {
            insert_env(&mut guest_config_env, ENTER_AS_ROOT_ENV, "1");
        }
        if spec.profile {
            insert_env(&mut guest_config_env, GUEST_PROFILE_ENV, "1");
        }
        if spec.log_level.enables_debug() {
            insert_env(&mut guest_config_env, GUEST_DEBUG_ENV, "1");
        }
        for (key, value) in spec.extra_env {
            guest_config_env.insert(key, value);
        }
        let argv = guest_init_argv(spec.guest_command, &spec.image_process_config.cmd);
        let workdir = spec
            .image_process_config
            .working_dir
            .as_deref()
            .filter(|working_dir| !working_dir.is_empty())
            .unwrap_or(WORKSPACE_TARGET)
            .to_owned();

        Ok(Self {
            task_rootfs: spec.task_rootfs.to_path_buf(),
            mounts: spec.mounts.to_vec(),
            disks: spec.disks,
            ram_mib,
            vcpus: spec.vcpus,
            log_level: spec.log_level,
            workdir,
            exec_path: spec.guest_init_exec.to_owned(),
            argv,
            env: vec![(
                KRUN_CONFIG_ENV.to_owned(),
                LOFTD_KRUN_CONFIG_PATH.to_owned(),
            )],
            guest_config_env: guest_config_env.into_iter().collect(),
        })
    }

    pub(crate) fn with_root_export(&self, root_export: PathBuf) -> Self {
        let mut config = self.clone();
        config.task_rootfs = root_export;
        config
    }

    #[cfg(test)]
    pub(crate) fn guest_config_env_contains(&self, name: &str, value: &str) -> bool {
        self.guest_config_env
            .iter()
            .any(|(key, actual)| key == name && actual == value)
    }

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

    fn serialize(&self) -> String {
        let mut out = String::new();
        push_field(
            &mut out,
            "task_rootfs",
            &self.task_rootfs.display().to_string(),
        );
        if let Ok(workspace) = workspace_mount(&self.mounts) {
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
        push_field(&mut out, "ram_mib", &self.ram_mib.to_string());
        push_field(&mut out, "vcpus", &self.vcpus.to_string());
        push_field(&mut out, "log_level", self.log_level.as_str());
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
        for (index, (key, value)) in self.guest_config_env.iter().enumerate() {
            push_field(
                &mut out,
                &format!("guest_env.{index}"),
                &format!("{key}={value}"),
            );
        }
        out
    }

    fn parse(text: &str) -> Result<Self> {
        let mut fields = BTreeMap::new();
        let mut argv = BTreeMap::new();
        let mut env = BTreeMap::new();
        let mut guest_config_env = BTreeMap::new();
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
                    | "ram_mib"
                    | "vcpus"
                    | "log_level"
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
        let log_level_text = required("log_level")?;
        let log_level = LogLevel::parse_name(&log_level_text)
            .ok_or_else(|| anyhow!("loftd launch config log_level is invalid"))?;
        let mounts = parse_mounts(&fields, mounts)?;
        Ok(Self {
            task_rootfs: PathBuf::from(required("task_rootfs")?),
            mounts,
            disks: disks
                .into_iter()
                .map(|(index, disk)| disk.finish(index))
                .collect::<Result<Vec<_>>>()?,
            ram_mib,
            vcpus,
            log_level,
            workdir: required("workdir")?,
            exec_path: required("exec_path")?,
            argv: argv.into_values().collect(),
            env: env.into_values().collect(),
            guest_config_env: guest_config_env.into_values().collect(),
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
        let workspace = workspace_mount(&mounts)?;
        if *workspace != legacy {
            anyhow::bail!(
                "loftd launch config workspace compatibility fields disagree with indexed mounts"
            );
        }
    }

    validate_mounts(&mounts)?;
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

fn workspace_mount(mounts: &[BindMount]) -> Result<&BindMount> {
    mounts
        .iter()
        .find(|mount| mount.target == WORKSPACE_TARGET)
        .ok_or_else(|| anyhow!("loftd launch config requires a {WORKSPACE_TARGET} mount"))
}

pub fn validate_mounts(mounts: &[BindMount]) -> Result<()> {
    if mounts.is_empty() {
        anyhow::bail!("loftd launch config requires at least one bind mount");
    }
    let mut tags = BTreeSet::new();
    let mut targets = BTreeSet::new();
    for mount in mounts {
        if mount.tag.trim().is_empty() {
            anyhow::bail!("loftd bind mount tag cannot be empty");
        }
        if !Path::new(&mount.target).is_absolute() {
            anyhow::bail!(
                "loftd bind mount target '{}' must be absolute",
                mount.target
            );
        }
        if mount.target.contains(".config/codex")
            || mount.source.to_string_lossy().contains(".config/codex")
        {
            anyhow::bail!("loftd bind mounts must not include .config/codex");
        }
        if !tags.insert(mount.tag.as_str()) {
            anyhow::bail!("loftd bind mount tag '{}' is duplicated", mount.tag);
        }
        if !targets.insert(mount.target.as_str()) {
            anyhow::bail!("loftd bind mount target '{}' is duplicated", mount.target);
        }
    }
    workspace_mount(mounts)?;
    Ok(())
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
    match mem_gib {
        Some(gib) => mem_gib_to_mib(gib),
        None => {
            let meminfo = fs::read_to_string(HOST_MEMINFO)
                .with_context(|| format!("failed to read host memory from {HOST_MEMINFO}"))?;
            default_ram_mib_from_meminfo(&meminfo)
        }
    }
}

fn default_ram_mib_from_meminfo(meminfo: &str) -> Result<u32> {
    let host_bytes = parse_meminfo_total_bytes(meminfo)?;
    let default_gib = default_mem_gib_from_host_bytes(host_bytes)?;
    mem_gib_to_mib(default_gib)
}

fn mem_gib_to_mib(gib: u32) -> Result<u32> {
    validate_mem_gib(gib)?;
    gib.checked_mul(1024)
        .ok_or_else(|| anyhow!("loftd --mem is too large for libkrun ram_mib"))
}

fn validate_mem_gib(gib: u32) -> Result<()> {
    if gib == 0 {
        anyhow::bail!("loftd --mem must be at least 1 GiB");
    }
    if gib > MAX_GIB_FOR_KRUN_RAM_MIB {
        anyhow::bail!("loftd --mem must be at most {MAX_GIB_FOR_KRUN_RAM_MIB} GiB");
    }
    Ok(())
}

fn default_mem_gib_from_host_bytes(host_bytes: u64) -> Result<u32> {
    let eighty_percent_bytes = (host_bytes / 5)
        .saturating_mul(4)
        .saturating_add((host_bytes % 5).saturating_mul(4) / 5);
    let default_gib = eighty_percent_bytes / BYTES_PER_GIB;

    if default_gib == 0 {
        anyhow::bail!("host memory is too small to derive a loftd --mem default of at least 1 GiB");
    }

    let default_gib = u32::try_from(default_gib)
        .context("host memory is too large to fit loftd --mem default")?;
    validate_mem_gib(default_gib)?;
    Ok(default_gib)
}

fn parse_meminfo_total_bytes(meminfo: &str) -> Result<u64> {
    let mem_total_line = meminfo
        .lines()
        .find(|line| line.starts_with("MemTotal:"))
        .ok_or_else(|| {
            anyhow!("host memory detection failed: MemTotal missing from {HOST_MEMINFO}")
        })?;

    let mut fields = mem_total_line.split_whitespace();
    let _label = fields.next();
    let value = fields
        .next()
        .ok_or_else(|| anyhow!("host memory detection failed: MemTotal value missing"))?;
    let unit = fields
        .next()
        .ok_or_else(|| anyhow!("host memory detection failed: MemTotal unit missing"))?;

    if unit != "kB" {
        anyhow::bail!("host memory detection failed: expected MemTotal in kB, got {unit}");
    }

    let kib = value
        .parse::<u64>()
        .with_context(|| format!("host memory detection failed: invalid MemTotal value {value}"))?;
    kib.checked_mul(KIB)
        .ok_or_else(|| anyhow!("host memory detection failed: MemTotal overflows bytes"))
}

fn bootstrap_env(
    image_env: &[String],
    required_env: Vec<(String, String)>,
) -> Result<BTreeMap<String, String>> {
    let mut env = BTreeMap::new();
    for entry in image_env {
        let (key, value) = entry
            .split_once('=')
            .ok_or_else(|| anyhow!("loftd image env entry '{entry}' is missing '='"))?;
        if key.is_empty() {
            anyhow::bail!("loftd image env entry '{entry}' has an empty key");
        }
        if is_allowed_image_env(key) {
            env.insert(key.to_owned(), value.to_owned());
        }
    }
    for (key, value) in required_env {
        env.insert(key, value);
    }
    Ok(env)
}

fn is_allowed_image_env(key: &str) -> bool {
    key == IMAGE_PATH_ENV || IMAGE_LOFTD_ENV_ALLOWLIST.contains(&key)
}

fn insert_env(env: &mut BTreeMap<String, String>, key: &str, value: &str) {
    env.insert(key.to_owned(), value.to_owned());
}

fn guest_init_argv(guest_command: &[String], image_cmd: &[String]) -> Vec<String> {
    let command = if guest_command.is_empty() {
        if image_cmd.is_empty() {
            vec!["fish".to_owned(), "-l".to_owned()]
        } else {
            image_cmd.to_vec()
        }
    } else {
        guest_command.to_vec()
    };

    ["enter", "--"]
        .into_iter()
        .map(str::to_owned)
        .chain(command)
        .collect()
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

fn guest_config_json(env: &[(String, String)]) -> String {
    let mut out = String::from("{\n  \"Env\": [");
    for (index, (key, value)) in env.iter().enumerate() {
        if index == 0 {
            out.push('\n');
        } else {
            out.push_str(",\n");
        }
        out.push_str("    \"");
        push_json_escaped(&mut out, key);
        out.push('=');
        push_json_escaped(&mut out, value);
        out.push('"');
    }
    if env.is_empty() {
        out.push_str("]\n}\n");
    } else {
        out.push_str("\n  ]\n}\n");
    }
    out
}

fn push_json_escaped(out: &mut String, value: &str) {
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            ch if ch.is_control() => out.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => out.push(ch),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;
    use tempfile::tempdir;

    fn test_mounts() -> Vec<BindMount> {
        vec![
            BindMount {
                source: Path::new("/workspace-src").to_path_buf(),
                tag: WORKSPACE_TAG.to_owned(),
                target: WORKSPACE_TARGET.to_owned(),
            },
            BindMount {
                source: Path::new("/home/host/.codex").to_path_buf(),
                tag: CODEX_TAG.to_owned(),
                target: CODEX_TARGET.to_owned(),
            },
            BindMount {
                source: Path::new("/home/host/.pi").to_path_buf(),
                tag: PI_TAG.to_owned(),
                target: PI_TARGET.to_owned(),
            },
            BindMount {
                source: Path::new("/state/project/cargo").to_path_buf(),
                tag: CARGO_TAG.to_owned(),
                target: CARGO_TARGET.to_owned(),
            },
            BindMount {
                source: Path::new("/state/sccache").to_path_buf(),
                tag: SCCACHE_TAG.to_owned(),
                target: SCCACHE_TARGET.to_owned(),
            },
        ]
    }

    #[test]
    fn launch_config_defaults_to_guest_init_enter_fish_shell() {
        let image_process_config = OciProcessConfig::default();
        let config = LaunchConfig::build_for_task(LaunchSpec {
            task_rootfs: Path::new("/state/task/rootfs"),
            mounts: &test_mounts(),
            guest_init_exec: "/nix/store/hash-loftd/bin/loftd-guest-init",
            guest_command: &[],
            image_process_config: &image_process_config,
            mem_gib: Some(4),
            log_level: LogLevel::Debug,
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
        assert_eq!(config.mounts[0].source, Path::new("/workspace-src"));
        assert_eq!(config.mounts[0].tag, "loftd-workspace");
        assert_eq!(config.mounts[0].target, "/workspace");
        assert_eq!(config.mounts.len(), 5);
        assert_eq!(config.ram_mib, 4096);
        assert_eq!(config.vcpus, 2);
        assert_eq!(config.log_level, LogLevel::Debug);
        assert_eq!(config.workdir, "/workspace");
        assert_eq!(
            config.exec_path,
            "/nix/store/hash-loftd/bin/loftd-guest-init"
        );
        assert_eq!(config.argv, ["enter", "--", "fish", "-l"]);
        assert_eq!(
            config.env,
            [("KRUN_CONFIG".to_owned(), "/.loftd_config.json".to_owned())]
        );
        assert!(config.guest_config_env_contains("LOFTD_HOST_UID", "1000"));
        assert!(config.guest_config_env_contains("LOFTD_HOST_GID", "1001"));
        assert!(
            config
                .guest_config_env
                .iter()
                .all(|(key, _)| !key.starts_with("LOFTD_MOUNT_"))
        );
        assert!(
            !config
                .guest_config_env
                .iter()
                .any(|(key, _)| key == "LOFTD_MOUNT_COUNT"
                    || key == "LOFTD_WORKSPACE_TAG"
                    || key == "LOFTD_WORKSPACE_TARGET")
        );
        assert!(config.guest_config_env_contains("SCCACHE_DIR", "/home/dev/.cache/sccache"));
        assert!(config.guest_config_env_contains("LOFTD_GUEST_PROFILE", "1"));
        assert!(config.guest_config_env_contains("LOFTD_GUEST_DEBUG", "1"));
        assert!(
            config
                .guest_config_env
                .iter()
                .all(|(key, value)| !key.starts_with("AGENTBOX_")
                    && !value.contains("/workspace-src")
                    && !value.contains(".config/codex"))
        );
    }

    #[test]
    fn launch_config_uses_explicit_guest_command() {
        let command = vec!["bash".to_owned(), "-lc".to_owned(), "echo ok".to_owned()];
        let image_process_config = OciProcessConfig::default();
        let config = LaunchConfig::build_for_task(LaunchSpec {
            task_rootfs: Path::new("/state/task/rootfs"),
            mounts: &test_mounts(),
            guest_init_exec: "/nix/store/hash-loftd/bin/loftd-guest-init",
            guest_command: &command,
            image_process_config: &image_process_config,
            mem_gib: Some(4),
            log_level: LogLevel::Off,
            profile: false,
            root: false,
            host_uid: 1000,
            host_gid: 1001,
            vcpus: 2,
            disks: Vec::new(),
            extra_env: Vec::new(),
        })
        .expect("launch config should build");

        assert_eq!(config.argv, ["enter", "--", "bash", "-lc", "echo ok"]);
    }

    #[test]
    fn launch_config_round_trips_through_hex_line_format() {
        let image_process_config = OciProcessConfig {
            env: vec!["PATH=/nix/store/fish/bin".to_owned()],
            cmd: vec!["fish".to_owned(), "-l".to_owned()],
            entrypoint: Vec::new(),
            working_dir: Some("/workspace/project".to_owned()),
        };
        let config = LaunchConfig::build_for_task(LaunchSpec {
            task_rootfs: Path::new("/state/task/rootfs"),
            mounts: &test_mounts(),
            guest_init_exec: "/nix/store/hash-loftd/bin/loftd-guest-init",
            guest_command: &[],
            image_process_config: &image_process_config,
            mem_gib: Some(2),
            log_level: LogLevel::Off,
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
        assert_eq!(parsed.mounts, test_mounts());
        assert_eq!(
            parsed.env,
            [("KRUN_CONFIG".to_owned(), "/.loftd_config.json".to_owned())]
        );
        assert!(parsed.guest_config_env_contains("LOFTD_ENTER_AS_ROOT", "1"));
        assert!(parsed.guest_config_env_contains("LOFTD_CONTAINERS_STORAGE", "1"));
        assert!(parsed.guest_config_env_contains("PATH", "/nix/store/fish/bin"));
        assert_eq!(parsed.argv, ["enter", "--", "fish", "-l"]);
        assert_eq!(parsed.workdir, "/workspace/project");
        assert_eq!(parsed.disks[0].id, "loftd-nix");
        assert_eq!(
            parsed.disks[1].path,
            Path::new("/state/loftd-containers.raw")
        );
    }

    #[test]
    fn launch_config_legacy_workspace_fields_fall_back_to_single_workspace_mount() {
        let mut text = String::new();
        push_field(&mut text, "task_rootfs", "/state/task/rootfs");
        push_field(&mut text, "workspace_source", "/workspace-src");
        push_field(&mut text, "workspace_tag", "loftd-workspace");
        push_field(&mut text, "workspace_target", "/workspace");
        push_field(&mut text, "ram_mib", "4096");
        push_field(&mut text, "vcpus", "2");
        push_field(&mut text, "log_level", "off");
        push_field(&mut text, "workdir", "/workspace");
        push_field(&mut text, "exec_path", "/loftd-guest-init");

        let parsed = LaunchConfig::parse(&text).expect("legacy config should parse");

        assert_eq!(
            parsed.mounts,
            [BindMount {
                source: Path::new("/workspace-src").to_path_buf(),
                tag: WORKSPACE_TAG.to_owned(),
                target: WORKSPACE_TARGET.to_owned(),
            }]
        );
    }

    #[test]
    fn launch_config_rejects_config_codex_mounts() {
        let mut mounts = test_mounts();
        mounts[1].source = Path::new("/home/host/.config/codex").to_path_buf();
        let image_process_config = OciProcessConfig::default();

        let err = LaunchConfig::build_for_task(LaunchSpec {
            task_rootfs: Path::new("/state/task/rootfs"),
            mounts: &mounts,
            guest_init_exec: "/nix/store/hash-loftd/bin/loftd-guest-init",
            guest_command: &[],
            image_process_config: &image_process_config,
            mem_gib: Some(4),
            log_level: LogLevel::Off,
            profile: false,
            root: false,
            host_uid: 1000,
            host_gid: 1001,
            vcpus: 2,
            disks: Vec::new(),
            extra_env: Vec::new(),
        })
        .expect_err("config codex source should be rejected");

        assert!(format!("{err:#}").contains(".config/codex"));
    }

    #[test]
    fn launch_config_rejects_unknown_missing_and_malformed_keys() {
        assert!(LaunchConfig::parse("unknown=61\n").is_err());
        assert!(LaunchConfig::parse("task_rootfs=6\n").is_err());
        assert!(LaunchConfig::parse("task_rootfs=2f\n").is_err());
    }

    #[test]
    fn default_ram_mib_floors_eighty_percent_of_host_memory_to_whole_gib() {
        let ten_gib_meminfo = "MemTotal:       10485760 kB\n";
        assert_eq!(
            default_ram_mib_from_meminfo(ten_gib_meminfo)
                .expect("10 GiB host should derive 8 GiB default"),
            8192
        );

        let two_gib_meminfo = "MemTotal:       2097152 kB\n";
        assert_eq!(
            default_ram_mib_from_meminfo(two_gib_meminfo)
                .expect("2 GiB host should derive 1 GiB default"),
            1024
        );
    }

    #[test]
    fn default_ram_mib_rejects_unusable_host_memory() {
        assert!(default_ram_mib_from_meminfo("MemTotal:       1048576 kB\n").is_err());
        assert!(default_ram_mib_from_meminfo("MemFree:        10485760 kB\n").is_err());
    }

    #[test]
    fn explicit_ram_mib_still_overrides_host_default() {
        assert_eq!(resolve_ram_mib(Some(4)).expect("explicit memory"), 4096);
        assert!(resolve_ram_mib(Some(0)).is_err());
    }

    #[test]
    fn libkrun_envp_stays_tiny_while_guest_config_env_is_allowlisted() {
        let image_process_config = OciProcessConfig {
            env: vec![
                "PATH=/ignored/first".to_owned(),
                "PATH=/nix/store/fish/bin".to_owned(),
                "OMX_API_BIN=/nix/store/host-only".to_owned(),
                "RUSTC_WRAPPER=/nix/store/sccache/bin/sccache".to_owned(),
                "LOFTD_HOST_UID=image".to_owned(),
                "LOFTD_FISH_CONFIG_SOURCE=/nix/store/fish-config".to_owned(),
                "LOFTD_STARSHIP_CONFIG_SOURCE=/nix/store/starship.toml".to_owned(),
                "LOFTD_GRAPHENE_HARDENED_MALLOC_LIB=/nix/store/libhardened_malloc.so".to_owned(),
                "LOFTD_REAL_PODMAN=/nix/store/podman/bin/podman".to_owned(),
                "LOFTD_UNRELATED_IMAGE_ENV=ignored".to_owned(),
                "LOFTD_CONTAINERS_STORAGE=image".to_owned(),
            ],
            ..OciProcessConfig::default()
        };

        let config = LaunchConfig::build_for_task(LaunchSpec {
            task_rootfs: Path::new("/state/task/rootfs"),
            mounts: &test_mounts(),
            guest_init_exec: "/nix/store/hash-loftd/bin/loftd-guest-init",
            guest_command: &[],
            image_process_config: &image_process_config,
            mem_gib: Some(4),
            log_level: LogLevel::Off,
            profile: false,
            root: false,
            host_uid: 1000,
            host_gid: 1001,
            vcpus: 2,
            disks: Vec::new(),
            extra_env: vec![("LOFTD_CONTAINERS_STORAGE".to_owned(), "disk".to_owned())],
        })
        .expect("launch config should build");

        assert_eq!(
            config.env,
            [("KRUN_CONFIG".to_owned(), "/.loftd_config.json".to_owned())]
        );
        assert!(config.guest_config_env_contains("PATH", "/nix/store/fish/bin"));
        assert!(config.guest_config_env_contains("LOFTD_HOST_UID", "1000"));
        assert!(config.guest_config_env_contains("LOFTD_CONTAINERS_STORAGE", "disk"));
        assert!(
            config.guest_config_env_contains("LOFTD_FISH_CONFIG_SOURCE", "/nix/store/fish-config")
        );
        assert!(
            config.guest_config_env_contains(
                "LOFTD_STARSHIP_CONFIG_SOURCE",
                "/nix/store/starship.toml"
            )
        );
        assert!(config.guest_config_env_contains(
            "LOFTD_GRAPHENE_HARDENED_MALLOC_LIB",
            "/nix/store/libhardened_malloc.so"
        ));
        assert!(
            config.guest_config_env_contains("LOFTD_REAL_PODMAN", "/nix/store/podman/bin/podman")
        );
        assert!(
            !config
                .guest_config_env
                .iter()
                .any(|(key, _)| key == "OMX_API_BIN"
                    || key == "RUSTC_WRAPPER"
                    || key == "LOFTD_UNRELATED_IMAGE_ENV")
        );
        assert_eq!(
            config
                .guest_config_env
                .iter()
                .filter(|(key, _)| key == "LOFTD_HOST_UID")
                .count(),
            1
        );
    }

    #[test]
    fn guest_debug_env_follows_effective_log_level() {
        let image_process_config = OciProcessConfig::default();
        let config = LaunchConfig::build_for_task(LaunchSpec {
            task_rootfs: Path::new("/state/task/rootfs"),
            mounts: &test_mounts(),
            guest_init_exec: "/nix/store/hash-loftd/bin/loftd-guest-init",
            guest_command: &[],
            image_process_config: &image_process_config,
            mem_gib: Some(4),
            log_level: LogLevel::Info,
            profile: false,
            root: false,
            host_uid: 1000,
            host_gid: 1001,
            vcpus: 2,
            disks: Vec::new(),
            extra_env: Vec::new(),
        })
        .expect("launch config should build");
        assert!(!config.guest_config_env_contains("LOFTD_GUEST_DEBUG", "1"));

        let config = LaunchConfig::build_for_task(LaunchSpec {
            task_rootfs: Path::new("/state/task/rootfs"),
            mounts: &test_mounts(),
            guest_init_exec: "/nix/store/hash-loftd/bin/loftd-guest-init",
            guest_command: &[],
            image_process_config: &image_process_config,
            mem_gib: Some(4),
            log_level: LogLevel::Trace,
            profile: false,
            root: false,
            host_uid: 1000,
            host_gid: 1001,
            vcpus: 2,
            disks: Vec::new(),
            extra_env: Vec::new(),
        })
        .expect("launch config should build");
        assert!(config.guest_config_env_contains("LOFTD_GUEST_DEBUG", "1"));
    }

    #[test]
    fn writes_loftd_config_json_under_task_rootfs() {
        let rootfs = tempdir().expect("tempdir should create");
        let image_process_config = OciProcessConfig {
            env: vec![
                "PATH=/nix/store/fish/bin".to_owned(),
                "LOFTD_FISH_CONFIG_SOURCE=/nix/store/config with \"quote\"".to_owned(),
            ],
            ..OciProcessConfig::default()
        };
        let config = LaunchConfig::build_for_task(LaunchSpec {
            task_rootfs: rootfs.path(),
            mounts: &test_mounts(),
            guest_init_exec: "/nix/store/hash-loftd/bin/loftd-guest-init",
            guest_command: &[],
            image_process_config: &image_process_config,
            mem_gib: Some(4),
            log_level: LogLevel::Off,
            profile: false,
            root: false,
            host_uid: 1000,
            host_gid: 1001,
            vcpus: 2,
            disks: Vec::new(),
            extra_env: vec![(
                "LOFTD_JSON_TEST".to_owned(),
                "line\nslash\\tab\t".to_owned(),
            )],
        })
        .expect("launch config should build");

        let path = config
            .write_guest_config_to_rootfs()
            .expect("guest config should write");
        let expected_path = rootfs.path().join(".loftd_config.json");
        assert_eq!(path, expected_path);

        let json = fs::read_to_string(expected_path).expect("guest config should be readable");
        assert!(json.starts_with("{\n  \"Env\": ["));
        assert!(json.contains("\"PATH=/nix/store/fish/bin\""));
        assert!(json.contains("LOFTD_FISH_CONFIG_SOURCE=/nix/store/config with \\\"quote\\\""));
        assert!(json.contains("LOFTD_JSON_TEST=line\\nslash\\\\tab\\t"));
    }

    #[test]
    fn malformed_image_env_is_rejected() {
        let missing_equals = OciProcessConfig {
            env: vec!["PATH".to_owned()],
            ..OciProcessConfig::default()
        };
        let empty_key = OciProcessConfig {
            env: vec!["=value".to_owned()],
            ..OciProcessConfig::default()
        };

        for image_process_config in [&missing_equals, &empty_key] {
            let err = LaunchConfig::build_for_task(LaunchSpec {
                task_rootfs: Path::new("/state/task/rootfs"),
                mounts: &test_mounts(),
                guest_init_exec: "/nix/store/hash-loftd/bin/loftd-guest-init",
                guest_command: &[],
                image_process_config,
                mem_gib: Some(4),
                log_level: LogLevel::Off,
                profile: false,
                root: false,
                host_uid: 1000,
                host_gid: 1001,
                vcpus: 2,
                disks: Vec::new(),
                extra_env: Vec::new(),
            })
            .expect_err("malformed image env should fail");
            assert!(err.to_string().contains("loftd image env entry"));
        }
    }

    #[test]
    fn image_cmd_is_used_before_default_shell_when_guest_command_is_empty() {
        let image_process_config = OciProcessConfig {
            cmd: vec!["bash".to_owned(), "-lc".to_owned(), "echo image".to_owned()],
            ..OciProcessConfig::default()
        };
        let config = LaunchConfig::build_for_task(LaunchSpec {
            task_rootfs: Path::new("/state/task/rootfs"),
            mounts: &test_mounts(),
            guest_init_exec: "/nix/store/hash-loftd/bin/loftd-guest-init",
            guest_command: &[],
            image_process_config: &image_process_config,
            mem_gib: Some(4),
            log_level: LogLevel::Off,
            profile: false,
            root: false,
            host_uid: 1000,
            host_gid: 1001,
            vcpus: 2,
            disks: Vec::new(),
            extra_env: Vec::new(),
        })
        .expect("launch config should build");

        assert_eq!(config.argv, ["enter", "--", "bash", "-lc", "echo image"]);
    }
}
