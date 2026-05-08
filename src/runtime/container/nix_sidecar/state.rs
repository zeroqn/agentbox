use anyhow::{anyhow, Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

use crate::runtime::container::nix_sidecar::types::{
    PodmanImageMountMode, SidecarPaths, SidecarState,
};

pub fn read_sidecar_state(paths: &SidecarPaths) -> Result<Option<SidecarState>> {
    if !paths.state_file.exists() {
        return Ok(None);
    }

    let contents = fs::read_to_string(&paths.state_file)
        .with_context(|| format!("failed to read '{}'", paths.state_file.display()))?;

    match parse_sidecar_state(&contents, &paths.state_file) {
        Ok(state) => Ok(Some(state)),
        Err(err) => {
            match fs::remove_file(&paths.state_file) {
                Ok(()) => {}
                Err(remove_err) if remove_err.kind() == std::io::ErrorKind::NotFound => {}
                Err(remove_err) => {
                    return Err(remove_err).with_context(|| {
                        format!(
                            "failed to remove stale sidecar state '{}' after parse error: {err:#}",
                            paths.state_file.display()
                        )
                    });
                }
            }
            eprintln!(
                "agentbox: ignored stale sidecar state '{}'; recreating sidecar stack ({err:#})",
                paths.state_file.display()
            );
            Ok(None)
        }
    }
}

pub fn write_sidecar_state(paths: &SidecarPaths, state: &SidecarState) -> Result<()> {
    let parent = paths.state_file.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)
        .with_context(|| format!("failed to create '{}'", parent.display()))?;

    let mount_mode = match state.mount_mode {
        PodmanImageMountMode::Direct => "direct",
        PodmanImageMountMode::Unshare => "unshare",
    };
    let proxy_port_line = match state.proxy_port {
        Some(port) => format!("proxy_port={}\n", port),
        None => String::new(),
    };
    let contents = format!(
        "image={}\nimage_id={}\nimage_mount_path={}\nsidecar_name={}\nmount_mode={}\n{proxy_port_line}",
        state.image,
        state.image_id,
        state.image_mount_path.display(),
        state.sidecar_name,
        mount_mode,
    );

    fs::write(&paths.state_file, contents)
        .with_context(|| format!("failed to write '{}'", paths.state_file.display()))
}

#[derive(Default)]
struct ParsedSidecarState {
    image: Option<String>,
    image_id: Option<String>,
    image_mount_path: Option<PathBuf>,
    sidecar_name: Option<String>,
    mount_mode: Option<PodmanImageMountMode>,
    proxy_port: Option<u16>,
    native_config: bool,
}

impl ParsedSidecarState {
    fn new() -> Self {
        Self {
            native_config: true,
            ..Self::default()
        }
    }

    fn into_sidecar_state(self, state_file: &Path) -> Result<SidecarState> {
        let Some(image) = self.image else {
            return Err(incomplete_state_error(state_file));
        };
        let Some(image_id) = self.image_id else {
            return Err(incomplete_state_error(state_file));
        };
        let Some(image_mount_path) = self.image_mount_path else {
            return Err(incomplete_state_error(state_file));
        };
        let Some(sidecar_name) = self.sidecar_name else {
            return Err(incomplete_state_error(state_file));
        };

        Ok(SidecarState {
            image,
            image_id,
            image_mount_path,
            sidecar_name,
            mount_mode: self.mount_mode.unwrap_or(PodmanImageMountMode::Direct),
            proxy_port: self.proxy_port,
            native_config: self.native_config,
        })
    }
}

fn parse_sidecar_state(contents: &str, state_file: &Path) -> Result<SidecarState> {
    let mut parsed = ParsedSidecarState::new();

    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let Some((key, value)) = trimmed.split_once('=') else {
            continue;
        };
        apply_state_entry(&mut parsed, key, value, state_file)?;
    }

    parsed.into_sidecar_state(state_file)
}

fn apply_state_entry(
    parsed: &mut ParsedSidecarState,
    key: &str,
    value: &str,
    state_file: &Path,
) -> Result<()> {
    match key {
        "image" => parsed.image = Some(value.to_owned()),
        "image_id" => parsed.image_id = Some(value.to_owned()),
        "image_mount_path" => parsed.image_mount_path = Some(PathBuf::from(value)),
        "sidecar_name" => parsed.sidecar_name = Some(value.to_owned()),
        "mount_mode" => parsed.mount_mode = Some(parse_mount_mode(value, state_file)?),
        "proxy_port" => parsed.proxy_port = Some(parse_proxy_port(value, state_file)?),
        "runtime_mode" if value != "native" => parsed.native_config = false,
        "network_mode" if value != "passt" => parsed.native_config = false,
        _ => {}
    }
    Ok(())
}

fn parse_mount_mode(value: &str, state_file: &Path) -> Result<PodmanImageMountMode> {
    match value {
        "direct" => Ok(PodmanImageMountMode::Direct),
        "unshare" => Ok(PodmanImageMountMode::Unshare),
        _ => Err(anyhow!(
            "unsupported mount_mode '{}' in '{}'",
            value,
            state_file.display()
        )),
    }
}

fn parse_proxy_port(value: &str, state_file: &Path) -> Result<u16> {
    value.parse::<u16>().map_err(|_| {
        anyhow!(
            "invalid proxy_port '{}' in '{}'",
            value,
            state_file.display()
        )
    })
}

fn incomplete_state_error(state_file: &Path) -> anyhow::Error {
    anyhow!("'{}' is incomplete", state_file.display())
}
