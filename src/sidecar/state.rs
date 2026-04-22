use anyhow::{anyhow, Context, Result};
use std::fs::{self, File};
use std::io::Write;
use std::path::Path;
use std::process;

use super::{name, PodmanImageMountMode, SidecarPaths, SidecarState};

const CURRENT_POINTER_KEY: &str = "generation";

pub fn migrate_legacy_state_if_needed(paths: &SidecarPaths) -> Result<()> {
    if !paths.legacy_state_file.exists() || has_modern_state(paths)? {
        return Ok(());
    }

    let contents = fs::read_to_string(&paths.legacy_state_file)
        .with_context(|| format!("failed to read '{}'", paths.legacy_state_file.display()))?;
    let state = parse_legacy_state(&contents, paths)?;

    write_generation_record(paths, &state)?;
    write_current_generation(paths, &state.generation)?;
    fs::rename(&paths.legacy_state_file, paths.migrated_legacy_state_file()).with_context(
        || {
            format!(
                "failed to rename legacy sidecar state '{}'",
                paths.legacy_state_file.display()
            )
        },
    )?;
    Ok(())
}

pub fn read_current_sidecar_state(paths: &SidecarPaths) -> Result<Option<SidecarState>> {
    let Some(generation) = read_current_generation(paths)? else {
        return Ok(None);
    };

    match read_generation_record(paths, &generation) {
        Ok(Some(state)) => Ok(Some(state)),
        Ok(None) => {
            eprintln!(
                "agentbox: current sidecar generation '{}' is missing its record; ignoring pointer",
                generation
            );
            Ok(None)
        }
        Err(err) => {
            eprintln!(
                "agentbox: current sidecar generation '{}' is corrupt; ignoring pointer ({err:#})",
                generation
            );
            Ok(None)
        }
    }
}

pub fn read_current_generation(paths: &SidecarPaths) -> Result<Option<String>> {
    if !paths.current_pointer.exists() {
        return Ok(None);
    }

    let contents = fs::read_to_string(&paths.current_pointer)
        .with_context(|| format!("failed to read '{}'", paths.current_pointer.display()))?;
    let generation = parse_required_key(&contents, CURRENT_POINTER_KEY, &paths.current_pointer)
        .map_err(|err| {
            eprintln!(
                "agentbox: current sidecar pointer '{}' is invalid; ignoring it ({err:#})",
                paths.current_pointer.display()
            );
            err
        })
        .ok();
    Ok(generation)
}

pub fn write_current_generation(paths: &SidecarPaths, generation: &str) -> Result<()> {
    let parent = paths
        .current_pointer
        .parent()
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)
        .with_context(|| format!("failed to create '{}'", parent.display()))?;

    write_atomic_file(
        &paths.current_pointer,
        &format!("{CURRENT_POINTER_KEY}={generation}\n"),
    )
}

pub fn clear_current_generation(paths: &SidecarPaths, generation: &str) -> Result<()> {
    let current = read_current_generation(paths)?;
    if current.as_deref() != Some(generation) {
        return Ok(());
    }

    match fs::remove_file(&paths.current_pointer) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err)
            .with_context(|| format!("failed to remove '{}'", paths.current_pointer.display())),
    }
}

pub fn read_generation_record(
    paths: &SidecarPaths,
    generation: &str,
) -> Result<Option<SidecarState>> {
    let record_file = paths.generation_record_file(generation);
    if !record_file.exists() {
        return Ok(None);
    }

    let contents = fs::read_to_string(&record_file)
        .with_context(|| format!("failed to read '{}'", record_file.display()))?;
    parse_generation_record(&contents, &record_file).map(Some)
}

pub fn list_generation_records(paths: &SidecarPaths) -> Result<Vec<SidecarState>> {
    if !paths.generations_dir.exists() {
        return Ok(Vec::new());
    }

    let mut records = Vec::new();
    for entry in fs::read_dir(&paths.generations_dir)
        .with_context(|| format!("failed to read '{}'", paths.generations_dir.display()))?
    {
        let entry = entry
            .with_context(|| format!("failed to inspect '{}'", paths.generations_dir.display()))?;
        let file_type = entry
            .file_type()
            .with_context(|| format!("failed to inspect '{}'", entry.path().display()))?;
        if !file_type.is_dir() {
            continue;
        }

        let generation = entry.file_name().to_string_lossy().to_string();
        match read_generation_record(paths, &generation) {
            Ok(Some(record)) => records.push(record),
            Ok(None) => {}
            Err(err) => eprintln!(
                "agentbox: ignored corrupt sidecar generation record '{}' ({err:#})",
                generation
            ),
        }
    }

    Ok(records)
}

pub fn write_generation_record(paths: &SidecarPaths, state: &SidecarState) -> Result<()> {
    let generation_dir = paths.generation_root_dir(&state.generation);
    fs::create_dir_all(&generation_dir)
        .with_context(|| format!("failed to create '{}'", generation_dir.display()))?;

    let mount_mode = match state.mount_mode {
        PodmanImageMountMode::Direct => "direct",
        PodmanImageMountMode::Unshare => "unshare",
    };
    let contents = format!(
        "generation={}\nimage={}\nimage_id={}\nimage_mount_path={}\nsidecar_name={}\nmount_mode={}\nmerged_dir={}\nupper_dir={}\nwork_dir={}\n",
        state.generation,
        state.image,
        state.image_id,
        state.image_mount_path.display(),
        state.sidecar_name,
        mount_mode,
        state.merged_dir.display(),
        state.upper_dir.display(),
        state.work_dir.display(),
    );

    write_atomic_file(&paths.generation_record_file(&state.generation), &contents)
}

pub fn remove_generation_record(paths: &SidecarPaths, generation: &str) -> Result<()> {
    let record_file = paths.generation_record_file(generation);
    match fs::remove_file(&record_file) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => {
            Err(err).with_context(|| format!("failed to remove '{}'", record_file.display()))
        }
    }
}

fn has_modern_state(paths: &SidecarPaths) -> Result<bool> {
    if paths.current_pointer.exists() {
        return Ok(true);
    }

    if !paths.generations_dir.exists() {
        return Ok(false);
    }

    Ok(fs::read_dir(&paths.generations_dir)
        .with_context(|| format!("failed to read '{}'", paths.generations_dir.display()))?
        .next()
        .is_some())
}

fn parse_generation_record(contents: &str, record_file: &Path) -> Result<SidecarState> {
    let generation = parse_required_key(contents, "generation", record_file)?;
    let image = parse_required_key(contents, "image", record_file)?;
    let image_id = parse_required_key(contents, "image_id", record_file)?;
    let image_mount_path = parse_required_path(contents, "image_mount_path", record_file)?;
    let sidecar_name = parse_required_key(contents, "sidecar_name", record_file)?;
    let mount_mode =
        parse_mount_mode(contents, record_file)?.unwrap_or(PodmanImageMountMode::Direct);
    let merged_dir = parse_required_path(contents, "merged_dir", record_file)?;
    let upper_dir = parse_required_path(contents, "upper_dir", record_file)?;
    let work_dir = parse_required_path(contents, "work_dir", record_file)?;

    Ok(SidecarState {
        generation,
        image,
        image_id,
        image_mount_path,
        sidecar_name,
        mount_mode,
        merged_dir,
        upper_dir,
        work_dir,
    })
}

fn parse_legacy_state(contents: &str, paths: &SidecarPaths) -> Result<SidecarState> {
    let image = parse_required_key(contents, "image", &paths.legacy_state_file)?;
    let image_id = parse_required_key(contents, "image_id", &paths.legacy_state_file)?;
    let image_mount_path =
        parse_required_path(contents, "image_mount_path", &paths.legacy_state_file)?;
    let sidecar_name = parse_required_key(contents, "sidecar_name", &paths.legacy_state_file)?;
    let mount_mode = parse_mount_mode(contents, &paths.legacy_state_file)?
        .unwrap_or(PodmanImageMountMode::Direct);
    let generation = name::derive_legacy_generation_id(&sidecar_name);

    Ok(SidecarState {
        generation,
        image,
        image_id,
        image_mount_path,
        sidecar_name,
        mount_mode,
        merged_dir: paths.legacy_merged_dir(),
        upper_dir: paths.legacy_upper_dir(),
        work_dir: paths.legacy_work_dir(),
    })
}

fn parse_required_key(contents: &str, key: &str, source: &Path) -> Result<String> {
    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        if let Some((parsed_key, value)) = trimmed.split_once('=') {
            if parsed_key == key {
                let value = value.trim();
                if value.is_empty() {
                    return Err(anyhow!(
                        "'{}' has an empty '{}' entry",
                        source.display(),
                        key
                    ));
                }
                return Ok(value.to_owned());
            }
        }
    }

    Err(anyhow!("'{}' is missing '{}'", source.display(), key))
}

fn parse_required_path(contents: &str, key: &str, source: &Path) -> Result<std::path::PathBuf> {
    Ok(parse_required_key(contents, key, source)?.into())
}

fn parse_mount_mode(contents: &str, source: &Path) -> Result<Option<PodmanImageMountMode>> {
    let mount_mode = match parse_optional_key(contents, "mount_mode") {
        None => return Ok(None),
        Some(value) => value,
    };

    Ok(Some(match mount_mode.as_str() {
        "direct" => PodmanImageMountMode::Direct,
        "unshare" => PodmanImageMountMode::Unshare,
        _ => {
            return Err(anyhow!(
                "unsupported mount_mode '{}' in '{}'",
                mount_mode,
                source.display()
            ))
        }
    }))
}

fn parse_optional_key(contents: &str, key: &str) -> Option<String> {
    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        if let Some((parsed_key, value)) = trimmed.split_once('=') {
            if parsed_key == key {
                return Some(value.trim().to_owned());
            }
        }
    }

    None
}

fn write_atomic_file(path: &Path, contents: &str) -> Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)
        .with_context(|| format!("failed to create '{}'", parent.display()))?;

    let temp_path = parent.join(format!(
        ".{}.tmp.{}",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("agentbox"),
        process::id()
    ));
    let mut file = File::create(&temp_path)
        .with_context(|| format!("failed to create '{}'", temp_path.display()))?;
    file.write_all(contents.as_bytes())
        .with_context(|| format!("failed to write '{}'", temp_path.display()))?;
    file.sync_all()
        .with_context(|| format!("failed to sync '{}'", temp_path.display()))?;
    fs::rename(&temp_path, path).with_context(|| {
        format!(
            "failed to rename '{}' to '{}'",
            temp_path.display(),
            path.display()
        )
    })
}
