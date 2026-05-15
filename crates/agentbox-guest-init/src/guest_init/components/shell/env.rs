use std::env;
use std::path::PathBuf;

use crate::guest_init::components::env::DEV_USER;
use crate::guest_init::components::home::identity::DevIdentity;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::guest_init) struct ShellEnvironment {
    pub(in crate::guest_init) vars: Vec<(String, String)>,
    pub(in crate::guest_init) tmpdir: PathBuf,
    pub(in crate::guest_init) runtime_dir: Option<PathBuf>,
}

pub(in crate::guest_init) fn derive(
    identity: &DevIdentity,
    containers_storage: bool,
) -> ShellEnvironment {
    let home = identity.home.display().to_string();
    let tmpdir = identity.home.join(".cache/tmp");
    let runtime_dir =
        containers_storage.then(|| PathBuf::from(format!("/run/user/{}", identity.uid)));
    let mut vars = vec![
        ("USER".to_owned(), DEV_USER.to_owned()),
        ("HOME".to_owned(), home.clone()),
        ("SHELL".to_owned(), identity.shell.display().to_string()),
        ("XDG_CONFIG_HOME".to_owned(), format!("{home}/.config")),
        ("XDG_DATA_HOME".to_owned(), format!("{home}/.local/share")),
        ("XDG_STATE_HOME".to_owned(), format!("{home}/.local/state")),
        ("XDG_CACHE_HOME".to_owned(), format!("{home}/.cache")),
        ("TMPDIR".to_owned(), tmpdir.display().to_string()),
    ];
    if let Some(runtime_dir) = &runtime_dir {
        vars.push((
            "XDG_RUNTIME_DIR".to_owned(),
            runtime_dir.display().to_string(),
        ));
    }
    if containers_storage {
        let path = env::var("PATH").unwrap_or_default();
        vars.push(("PATH".to_owned(), format!("/run/agentbox/idmap-bin:{path}")));
        if let Some(runtime_dir) = &runtime_dir {
            vars.push((
                "DOCKER_HOST".to_owned(),
                format!("unix://{}/docker/docker.sock", runtime_dir.display()),
            ));
        }
    }
    ShellEnvironment {
        vars,
        tmpdir,
        runtime_dir,
    }
}

pub(in crate::guest_init) fn export(env_contract: &ShellEnvironment) {
    for (key, value) in &env_contract.vars {
        // SAFETY: guest-init builds and exports the shell environment during
        // single-threaded bootstrap immediately before replacing the process.
        unsafe { env::set_var(key, value) };
    }
}

#[cfg(test)]
#[path = "env_tests.rs"]
mod tests;
