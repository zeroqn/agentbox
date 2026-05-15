mod cargo;
mod codex;
mod sccache;

use anyhow::Result;
use std::path::Path;

use crate::podman::run::{RunArgOwner, RunSpec};
use crate::podman::volume::format_mount_arg;
use crate::state::StateLayout;
use crate::{CONTAINER_SCCACHE_DIR, CONTAINER_WORKDIR};

pub(crate) const WORKSPACE_VOLUME_OWNER: RunArgOwner = RunArgOwner::new("runtime.volume.workspace");
pub(crate) const CODEX_VOLUME_OWNER: RunArgOwner = RunArgOwner::new("runtime.volume.codex");
pub(crate) const CARGO_VOLUME_OWNER: RunArgOwner = RunArgOwner::new("runtime.volume.cargo");
pub(crate) const SCCACHE_VOLUME_OWNER: RunArgOwner = RunArgOwner::new("runtime.volume.sccache");

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskVolumeMounts {
    pub workspace: String,
    pub codex: String,
    pub cargo: String,
    pub sccache: String,
}

pub fn prepare_task_volumes(cwd: &Path, state_layout: &StateLayout) -> Result<TaskVolumeMounts> {
    Ok(TaskVolumeMounts {
        workspace: format_mount_arg(cwd, CONTAINER_WORKDIR)?,
        codex: codex::prepare()?,
        cargo: cargo::prepare(state_layout.root_dir())?,
        sccache: sccache::prepare(&state_layout.sccache_dir())?,
    })
}

pub fn append_task_volumes(run: &mut RunSpec, mounts: &TaskVolumeMounts) {
    append_workspace(run, &mounts.workspace);
    append_codex(run, &mounts.codex);
    append_cargo(run, &mounts.cargo);
    append_sccache(run, &mounts.sccache);
}

fn append_workspace(run: &mut RunSpec, mount: &str) {
    run.option(WORKSPACE_VOLUME_OWNER, "--volume", mount);
}

fn append_codex(run: &mut RunSpec, mount: &str) {
    run.option(CODEX_VOLUME_OWNER, "--volume", mount);
}

fn append_cargo(run: &mut RunSpec, mount: &str) {
    run.option(CARGO_VOLUME_OWNER, "--volume", mount);
}

fn append_sccache(run: &mut RunSpec, mount: &str) {
    run.option(SCCACHE_VOLUME_OWNER, "--volume", mount);
    run.option(
        SCCACHE_VOLUME_OWNER,
        "--env",
        format!("SCCACHE_DIR={CONTAINER_SCCACHE_DIR}"),
    );
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    #[test]
    fn prepare_codex_volume_creates_dot_codex_under_home() {
        let dir = tempfile::tempdir().expect("tempdir should be created");

        let mount = crate::runtime::components::volumes::codex::prepare_at(dir.path())
            .expect("codex mount should be prepared");

        assert_eq!(
            mount,
            format!(
                "{}:{}",
                dir.path().join(".codex").display(),
                crate::CONTAINER_CODEX_DIR
            )
        );
        assert!(dir.path().join(".codex").is_dir());
    }

    #[test]
    fn prepare_cargo_volume_creates_cargo_under_state_root() {
        let dir = tempfile::tempdir().expect("tempdir should be created");
        let state_root = dir.path().join("state").join("agentbox").join("project");

        let mount = crate::runtime::components::volumes::cargo::prepare(&state_root)
            .expect("cargo mount should be prepared");

        assert_eq!(
            mount,
            format!(
                "{}:{}",
                state_root.join("cargo").display(),
                crate::CONTAINER_CARGO_DIR
            )
        );
        assert!(state_root.join("cargo").is_dir());
    }

    #[test]
    fn prepare_sccache_volume_creates_shared_sccache_under_app_root() {
        let dir = tempfile::tempdir().expect("tempdir should be created");
        let sccache_dir = dir.path().join("state").join("agentbox").join("sccache");

        let mount = crate::runtime::components::volumes::sccache::prepare(&sccache_dir)
            .expect("sccache mount should be prepared");

        assert_eq!(
            mount,
            format!("{}:{}", sccache_dir.display(), crate::CONTAINER_SCCACHE_DIR)
        );
        assert!(sccache_dir.is_dir());
    }

    #[test]
    fn task_volume_mounts_append_in_component_order() {
        let mut run = crate::podman::run::RunSpec::new();
        let mounts = crate::runtime::components::volumes::TaskVolumeMounts {
            workspace: "/tmp/project:/workspace".to_owned(),
            codex: "/home/alice/.codex:/home/dev/.codex".to_owned(),
            cargo: "/tmp/state/agentbox/project/cargo:/home/dev/.cargo".to_owned(),
            sccache: "/tmp/state/agentbox/sccache:/home/dev/.cache/sccache".to_owned(),
        };

        crate::runtime::components::volumes::append_task_volumes(&mut run, &mounts);
        let args = run.render();

        assert!(args.contains_option_from(
            crate::runtime::components::volumes::WORKSPACE_VOLUME_OWNER,
            "--volume",
            "/tmp/project:/workspace"
        ));
        assert!(args.contains_option_from(
            crate::runtime::components::volumes::SCCACHE_VOLUME_OWNER,
            "--env",
            &format!("SCCACHE_DIR={}", crate::CONTAINER_SCCACHE_DIR)
        ));
    }

    #[test]
    fn workspace_volume_is_part_of_task_volume_mounts() {
        let mounts = crate::runtime::components::volumes::TaskVolumeMounts {
            workspace: crate::podman::volume::format_mount_arg(
                Path::new("/tmp/project"),
                crate::CONTAINER_WORKDIR,
            )
            .expect("workspace mount should format"),
            codex: String::new(),
            cargo: String::new(),
            sccache: String::new(),
        };

        assert_eq!(mounts.workspace, "/tmp/project:/workspace");
    }
}
