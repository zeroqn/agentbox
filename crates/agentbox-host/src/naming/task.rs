use std::fs::File;
use std::io::{self, Read};
use std::path::Path;
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::naming::derive_workspace_slug;

const TASK_HOSTNAME_SUFFIX: &str = "agentbox";

pub(crate) fn derive_task_hostname(cwd: &Path) -> String {
    format!("{}-{TASK_HOSTNAME_SUFFIX}", derive_workspace_slug(cwd))
}

pub(crate) fn derive_task_container_name(cwd: &Path) -> String {
    derive_task_container_name_with_suffix(cwd, &derive_task_container_name_suffix())
}

fn derive_task_container_name_with_suffix(cwd: &Path, suffix: &str) -> String {
    format!("{}-{suffix}", derive_workspace_slug(cwd))
}

fn derive_task_container_name_suffix() -> String {
    if let Ok(suffix) = random_task_container_name_suffix() {
        return suffix;
    }

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();

    format!("{:x}-{timestamp:x}", process::id())
}

fn random_task_container_name_suffix() -> io::Result<String> {
    let mut bytes = [0; 8];
    File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    Ok(format!("{:016x}", u64::from_ne_bytes(bytes)))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::naming::task::{derive_task_container_name_with_suffix, derive_task_hostname};

    #[test]
    fn task_hostname_uses_current_directory_name() {
        assert_eq!(
            derive_task_hostname(Path::new("/tmp/project")),
            "project-agentbox"
        );
    }

    #[test]
    fn task_hostname_sanitizes_current_directory_name() {
        assert_eq!(
            derive_task_hostname(Path::new("/tmp/My repo.name!")),
            "my-repo-name-agentbox"
        );
    }

    #[test]
    fn task_hostname_falls_back_when_directory_name_has_no_slug_chars() {
        assert_eq!(
            derive_task_hostname(Path::new("/tmp/!!!")),
            "workspace-agentbox"
        );
    }

    #[test]
    fn task_container_name_prefixes_unique_suffix_with_current_directory_name() {
        assert_eq!(
            derive_task_container_name_with_suffix(
                Path::new("/tmp/My repo.name!"),
                "random-suffix"
            ),
            "my-repo-name-random-suffix"
        );
    }

    #[test]
    fn task_container_name_falls_back_when_directory_name_has_no_slug_chars() {
        assert_eq!(
            derive_task_container_name_with_suffix(Path::new("/tmp/!!!"), "random-suffix"),
            "workspace-random-suffix"
        );
    }
}
