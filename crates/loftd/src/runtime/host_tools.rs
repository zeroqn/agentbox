//! Host runtime helper resolution for loftd.
//!
//! Nix installs loftd's runtime helpers under `$out/libexec/loftd-helpers` so
//! `$out/bin/loftd` can stay a raw ELF instead of a shell wrapper. Development
//! builds still fall back to ordinary `PATH` lookups.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

const HELPER_BINARY_DIR_ENV: &str = "LOFTD_HELPER_BINARY_DIR";
const HELPER_BINARY_DIR: &str = "libexec/loftd-helpers";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RuntimeTool {
    Buildah,
    Btrfs,
    MkfsBtrfs,
    Blkid,
    Pasta,
    Passt,
    Strace,
}

impl RuntimeTool {
    pub(crate) fn basename(self) -> &'static str {
        match self {
            Self::Buildah => "buildah",
            Self::Btrfs => "btrfs",
            Self::MkfsBtrfs => "mkfs.btrfs",
            Self::Blkid => "blkid",
            Self::Pasta => "pasta",
            Self::Passt => "passt",
            Self::Strace => "strace",
        }
    }

    fn override_env(self) -> &'static str {
        match self {
            Self::Buildah => "LOFTD_BUILDAH",
            Self::Btrfs => "LOFTD_BTRFS",
            Self::MkfsBtrfs => "LOFTD_MKFS_BTRFS",
            Self::Blkid => "LOFTD_BLKID",
            Self::Pasta => "LOFTD_PASTA",
            Self::Passt => "LOFTD_PASST",
            Self::Strace => "LOFTD_STRACE",
        }
    }
}

pub(crate) fn runtime_tool_program(tool: RuntimeTool) -> OsString {
    runtime_tool_program_with(
        tool,
        std::env::var_os(tool.override_env()),
        std::env::var_os(HELPER_BINARY_DIR_ENV),
        std::env::current_exe().ok(),
    )
}

fn runtime_tool_program_with(
    tool: RuntimeTool,
    tool_override: Option<OsString>,
    helper_dir_override: Option<OsString>,
    current_exe: Option<PathBuf>,
) -> OsString {
    if let Some(program) = nonempty_os_string(tool_override) {
        return program;
    }

    if let Some(helper_dir) = nonempty_os_string(helper_dir_override) {
        return PathBuf::from(helper_dir)
            .join(tool.basename())
            .into_os_string();
    }

    if let Some(exe) = current_exe
        && let Some(program) = package_helper_path_for_exe(&exe, tool)
        && program.is_file()
    {
        return program.into_os_string();
    }

    OsString::from(tool.basename())
}

fn nonempty_os_string(value: Option<OsString>) -> Option<OsString> {
    value.filter(|value| !value.as_os_str().is_empty())
}

pub(crate) fn package_helper_path_for_exe(exe: &Path, tool: RuntimeTool) -> Option<PathBuf> {
    package_root_from_exe(exe).map(|root| root.join(HELPER_BINARY_DIR).join(tool.basename()))
}

pub(crate) fn package_root_from_exe(exe: &Path) -> Option<PathBuf> {
    let executable_dir = exe.parent()?;
    if matches!(
        executable_dir.file_name().and_then(OsStr::to_str),
        Some("bin" | "libexec")
    ) {
        return executable_dir.parent().map(Path::to_path_buf);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_helper_paths_are_derived_from_bin_loftd_without_store_hashes() {
        assert_eq!(
            package_root_from_exe(Path::new("/nix/store/hash-agentbox/bin/loftd")),
            Some(PathBuf::from("/nix/store/hash-agentbox"))
        );
        assert_eq!(
            package_helper_path_for_exe(
                Path::new("/nix/store/hash-agentbox/bin/loftd"),
                RuntimeTool::Buildah
            ),
            Some(PathBuf::from(
                "/nix/store/hash-agentbox/libexec/loftd-helpers/buildah"
            ))
        );
    }

    #[test]
    fn legacy_libexec_executable_shape_still_resolves_package_root() {
        assert_eq!(
            package_helper_path_for_exe(
                Path::new("/nix/store/hash-agentbox/libexec/loftd"),
                RuntimeTool::Passt
            ),
            Some(PathBuf::from(
                "/nix/store/hash-agentbox/libexec/loftd-helpers/passt"
            ))
        );
    }

    #[test]
    fn explicit_tool_override_wins_before_helper_dir_and_package_paths() {
        assert_eq!(
            runtime_tool_program_with(
                RuntimeTool::Btrfs,
                Some(OsString::from("/custom/btrfs")),
                Some(OsString::from("/helpers")),
                Some(PathBuf::from("/nix/store/hash-agentbox/bin/loftd")),
            ),
            OsString::from("/custom/btrfs")
        );
    }

    #[test]
    fn explicit_helper_dir_wins_before_package_relative_paths() {
        assert_eq!(
            runtime_tool_program_with(
                RuntimeTool::MkfsBtrfs,
                None,
                Some(OsString::from("/helpers")),
                Some(PathBuf::from("/nix/store/hash-agentbox/bin/loftd")),
            ),
            OsString::from("/helpers/mkfs.btrfs")
        );
    }

    #[test]
    fn missing_package_helper_falls_back_to_bare_program_for_development() {
        assert_eq!(
            runtime_tool_program_with(
                RuntimeTool::Blkid,
                None,
                None,
                Some(PathBuf::from("/nix/store/hash-agentbox/bin/loftd")),
            ),
            OsString::from("blkid")
        );
    }
}
