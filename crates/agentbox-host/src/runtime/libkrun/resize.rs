use anyhow::{Context, Result};
use std::env;
use std::path::{Path, PathBuf};
use std::process::{ExitCode, Stdio};

use crate::CONTAINER_TMP_TMPFS;
use crate::cli::{CommonOptions, LibkrunOptions, LibkrunResizeOptions, LibkrunResizeTarget};
use crate::naming::{derive_task_container_name, derive_task_hostname};
use crate::podman::command::{run_podman, run_podman_output};
use crate::podman::run::{CORE, RunArgs, RunSpec};
use crate::runtime::components::identity;
use crate::runtime::libkrun::components::disk::containers::podman as containers_podman;
use crate::runtime::libkrun::components::disk::containers::raw_image::RAW_CONTAINER_DISK_SPEC;
use crate::runtime::libkrun::components::disk::nix::podman as nix_podman;
use crate::runtime::libkrun::components::disk::nix::raw_image::RAW_NIX_DISK_SPEC;
use crate::runtime::libkrun::components::disk::raw_btrfs::{
    HostRawImageCommandRunner, RawBtrfsDisk, RawDiskSpec, RawImageCommandRunner,
    grow_existing_path_with_runner,
};
use crate::runtime::libkrun::components::guest_init::{
    GuestInitOverrideMount, resolve_guest_init_override_mount, resolve_libkrun_guest_init_target,
};
use crate::runtime::libkrun::components::{cpu, guest_init, host_identity, memory, network, oci};
use crate::state::resolve_state_layout;

const GIB: u64 = 1024 * 1024 * 1024;
const TIB: u64 = 1024 * GIB;

pub(crate) fn parse_raw_image_size_arg(value: &str) -> std::result::Result<u64, String> {
    let (digits, multiplier) = raw_image_size_parts(value).ok_or_else(|| {
        "must be a positive integer size like 128, 128G, 128GiB, or 2TiB".to_owned()
    })?;
    let units = digits
        .parse::<u64>()
        .map_err(|_| "size must start with a positive integer".to_owned())?;
    if units == 0 {
        return Err("size must be greater than zero".to_owned());
    }
    units
        .checked_mul(multiplier)
        .ok_or_else(|| "size is too large".to_owned())
}

fn raw_image_size_parts(value: &str) -> Option<(&str, u64)> {
    let value = value.trim();
    for (suffix, multiplier) in [
        ("GiB", GIB),
        ("GIB", GIB),
        ("gib", GIB),
        ("G", GIB),
        ("g", GIB),
        ("TiB", TIB),
        ("TIB", TIB),
        ("tib", TIB),
        ("T", TIB),
        ("t", TIB),
    ] {
        if let Some(digits) = value.strip_suffix(suffix) {
            return (!digits.is_empty() && digits.chars().all(|ch| ch.is_ascii_digit()))
                .then_some((digits, multiplier));
        }
    }

    (!value.is_empty() && value.chars().all(|ch| ch.is_ascii_digit())).then_some((value, GIB))
}

pub(crate) fn run(
    common: CommonOptions,
    run_options: LibkrunOptions,
    resize_options: LibkrunResizeOptions,
) -> Result<ExitCode> {
    let cwd = env::current_dir()
        .context("failed to resolve current directory")?
        .canonicalize()
        .context("failed to canonicalize current directory")?;
    let image = crate::cli::resolve_image(common.image.as_deref(), common.pull_latest)?;
    let state_layout = resolve_state_layout(&cwd)?;
    let target = resize_options.target;
    let target_path = managed_disk_path(state_layout.root_dir(), target);

    ensure_no_live_raw_disk_users(&target_path, &HostPodmanOutputRunner)?;
    let image_guest_init_target = resolve_libkrun_guest_init_target(&image)?;
    let guest_init_override = run_options
        .guest_init
        .as_deref()
        .map(|path| resolve_guest_init_override_mount(path, &image))
        .transpose()?;
    let guest_init_target = guest_init_override
        .as_ref()
        .map(|mount| mount.target.as_str())
        .unwrap_or(&image_guest_init_target);

    let disk = grow_managed_disk_with_runner(
        state_layout.root_dir(),
        target,
        resize_options.size_bytes,
        &HostRawImageCommandRunner,
    )?;

    let task_container_name = format!("{}-resize", derive_task_container_name(&cwd));
    let task_hostname = derive_task_hostname(&cwd);
    let ram_mib = memory::resolve_libkrun_ram_mib(run_options.mem_gib)?;
    let cpu_count = cpu::resolve_libkrun_cpu_count()?;

    let status = run_podman(
        build_libkrun_resize_podman_args(LibkrunResizePodmanSpec {
            image: &image,
            container_name: &task_container_name,
            hostname: &task_hostname,
            target,
            disk: &disk,
            ram_mib,
            cpu_count,
            tsi: run_options.tsi,
            guest_profile: common.profile,
            guest_debug: common.debug,
            guest_init_target,
            guest_init_override: guest_init_override.as_ref(),
        })?,
        Stdio::null(),
        Stdio::inherit(),
        Stdio::inherit(),
        "failed to start podman libkrun resize task",
    )
    .with_context(|| retry_after_guest_failure_message(target, &disk.path, None))?;

    let code = status.code().unwrap_or(1);
    if !status.success() {
        eprintln!(
            "{}",
            retry_after_guest_failure_message(target, &disk.path, Some(code))
        );
    }

    Ok(ExitCode::from(u8::try_from(code).unwrap_or(1)))
}

fn managed_disk_path(state_root: &Path, target: LibkrunResizeTarget) -> PathBuf {
    state_root.join(target_spec(target).file_name)
}

pub(crate) fn grow_managed_disk_with_runner(
    state_root: &Path,
    target: LibkrunResizeTarget,
    target_size_bytes: u64,
    runner: &impl RawImageCommandRunner,
) -> Result<RawBtrfsDisk> {
    let spec = target_spec(target);
    let path = managed_disk_path(state_root, target);
    grow_existing_path_with_runner(&path, spec, target_size_bytes, runner)
}

fn target_spec(target: LibkrunResizeTarget) -> &'static RawDiskSpec {
    match target {
        LibkrunResizeTarget::Nix => &RAW_NIX_DISK_SPEC,
        LibkrunResizeTarget::Containers => &RAW_CONTAINER_DISK_SPEC,
    }
}

fn retry_after_guest_failure_message(
    target: LibkrunResizeTarget,
    path: &Path,
    code: Option<i32>,
) -> String {
    let code = code
        .map(|code| format!(" exited with status {code}"))
        .unwrap_or_else(|| " failed to start".to_owned());
    format!(
        "agentbox: libkrun resize guest step for {}{} after host raw image '{}' was enlarged; fix the reported guest error and rerun the same resize command. The raw image will not be shrunk or reset automatically.",
        target.as_str(),
        code,
        path.display()
    )
}

trait PodmanOutputRunner {
    fn output(&self, args: Vec<String>, context: &str) -> Result<String>;
}

#[derive(Debug, Clone, Copy)]
struct HostPodmanOutputRunner;

impl PodmanOutputRunner for HostPodmanOutputRunner {
    fn output(&self, args: Vec<String>, context: &str) -> Result<String> {
        run_podman_output(args, context)
    }
}

fn ensure_no_live_raw_disk_users(path: &Path, runner: &impl PodmanOutputRunner) -> Result<()> {
    let users = live_raw_disk_users(path, runner)?;
    if !users.is_empty() {
        anyhow::bail!(
            "refusing to resize libkrun raw image '{}' while running Podman container(s) use it: {}",
            path.display(),
            users.join(", ")
        );
    }
    Ok(())
}

fn live_raw_disk_users(path: &Path, runner: &impl PodmanOutputRunner) -> Result<Vec<String>> {
    let ps = runner.output(
        vec!["ps".to_owned(), "--format".to_owned(), "{{.ID}}".to_owned()],
        "failed to list running Podman containers before libkrun resize",
    )?;
    let target = path.to_string_lossy();
    let mut users = Vec::new();
    for id in ps.lines().map(str::trim).filter(|line| !line.is_empty()) {
        let annotations = runner.output(
            vec![
                "inspect".to_owned(),
                "--format".to_owned(),
                "{{range $key, $value := .Config.Annotations}}{{printf \"%s=%s\\n\" $key $value}}{{end}}"
                    .to_owned(),
                id.to_owned(),
            ],
            "failed to inspect running Podman container annotations before libkrun resize",
        )?;
        if annotations_use_raw_disk(&annotations, &target) {
            users.push(id.to_owned());
        }
    }
    Ok(users)
}

fn annotations_use_raw_disk(annotations: &str, target_path: &str) -> bool {
    annotations.lines().any(|line| {
        let Some((key, value)) = line.split_once('=') else {
            return false;
        };
        key.starts_with("krun.disk.") && key.ends_with(".path") && value == target_path
    })
}

pub(crate) struct LibkrunResizePodmanSpec<'a> {
    pub(crate) image: &'a str,
    pub(crate) container_name: &'a str,
    pub(crate) hostname: &'a str,
    pub(crate) target: LibkrunResizeTarget,
    pub(crate) disk: &'a RawBtrfsDisk,
    pub(crate) ram_mib: u32,
    pub(crate) cpu_count: Option<u32>,
    pub(crate) tsi: bool,
    pub(crate) guest_profile: bool,
    pub(crate) guest_debug: bool,
    pub(crate) guest_init_target: &'a str,
    pub(crate) guest_init_override: Option<&'a GuestInitOverrideMount>,
}

pub(crate) fn build_libkrun_resize_podman_args(
    spec: LibkrunResizePodmanSpec<'_>,
) -> Result<Vec<String>> {
    Ok(build_libkrun_resize_run_args(spec)?.into_vec())
}

pub(crate) fn build_libkrun_resize_run_args(spec: LibkrunResizePodmanSpec<'_>) -> Result<RunArgs> {
    let mut run = RunSpec::new();

    run.args(CORE, ["run", "--rm"]);
    run.option(CORE, "--name", spec.container_name);
    run.option(CORE, "--hostname", spec.hostname);
    identity::append_userns_keep_id(&mut run);
    host_identity::append_root_user(&mut run);
    oci::append_oci_args(&mut run);
    memory::append_ram_annotation(&mut run, spec.ram_mib);
    append_target_disk_args(&mut run, spec.target, spec.disk);
    cpu::append_cpu_annotation(&mut run, spec.cpu_count);
    network::append_mode_args(&mut run, spec.tsi);
    crate::runtime::components::diagnostics::append_guest_diagnostics(
        &mut run,
        spec.guest_profile,
        spec.guest_debug,
    );
    guest_init::append_guest_init_override_args(&mut run, spec.guest_init_override);
    run.option(CORE, "--tmpfs", CONTAINER_TMP_TMPFS);
    run.option(CORE, "--entrypoint", spec.guest_init_target);
    run.arg(CORE, spec.image);
    run.args(
        CORE,
        ["libkrun", "resize", "--target", spec.target.as_str()],
    );

    Ok(run.render())
}

fn append_target_disk_args(run: &mut RunSpec, target: LibkrunResizeTarget, disk: &RawBtrfsDisk) {
    match target {
        LibkrunResizeTarget::Nix => {
            nix_podman::append_disk_annotations(run, disk);
            nix_podman::append_disk_env(run, disk);
        }
        LibkrunResizeTarget::Containers => {
            containers_podman::append_disk_annotations(run, disk);
            containers_podman::append_disk_env(run, disk);
        }
    }
}

impl LibkrunResizeTarget {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Nix => "nix",
            Self::Containers => "containers",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::libkrun::components::disk::raw_btrfs::{
        RawBtrfsDiskStatus, test_support::FakeRunner,
    };
    use anyhow::anyhow;
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::fs::{self, File};
    use tempfile::tempdir;

    #[test]
    fn raw_size_parser_accepts_binary_gib_and_tib_syntax() {
        assert_eq!(parse_raw_image_size_arg("128").unwrap(), 128 * GIB);
        assert_eq!(parse_raw_image_size_arg("128G").unwrap(), 128 * GIB);
        assert_eq!(parse_raw_image_size_arg("128GiB").unwrap(), 128 * GIB);
        assert_eq!(parse_raw_image_size_arg("2T").unwrap(), 2 * TIB);
        assert_eq!(parse_raw_image_size_arg("2TiB").unwrap(), 2 * TIB);
    }

    #[test]
    fn raw_size_parser_rejects_invalid_or_zero_sizes() {
        for value in ["0", "", "1.5G", "8M", "abc", "G"] {
            assert!(parse_raw_image_size_arg(value).is_err(), "{value}");
        }
    }

    #[test]
    fn grow_managed_disk_expands_existing_btrfs_sparse_file() {
        let temp = tempdir().unwrap();
        let path = temp.path().join(RAW_NIX_DISK_SPEC.file_name);
        let file = File::create(&path).unwrap();
        file.set_len(RAW_NIX_DISK_SPEC.size_bytes).unwrap();
        drop(file);
        let runner = FakeRunner::with_probe("btrfs");
        let target_size = RAW_NIX_DISK_SPEC.size_bytes + GIB;

        let disk = grow_managed_disk_with_runner(
            temp.path(),
            LibkrunResizeTarget::Nix,
            target_size,
            &runner,
        )
        .unwrap();

        assert_eq!(disk.path, path);
        assert_eq!(disk.size_bytes, target_size);
        assert_eq!(disk.status, RawBtrfsDiskStatus::Reused);
        assert_eq!(disk.path.metadata().unwrap().len(), target_size);
        assert_eq!(runner.mkfs_call_count(), 0);
    }

    #[test]
    fn grow_managed_disk_rejects_shrink_and_equal_size() {
        for target_size in [
            RAW_NIX_DISK_SPEC.size_bytes - 1,
            RAW_NIX_DISK_SPEC.size_bytes,
        ] {
            let temp = tempdir().unwrap();
            let path = temp.path().join(RAW_NIX_DISK_SPEC.file_name);
            let file = File::create(&path).unwrap();
            file.set_len(RAW_NIX_DISK_SPEC.size_bytes).unwrap();
            drop(file);
            let runner = FakeRunner::with_probe("btrfs");

            let err = grow_managed_disk_with_runner(
                temp.path(),
                LibkrunResizeTarget::Nix,
                target_size,
                &runner,
            )
            .unwrap_err();

            assert!(err.to_string().contains("must be greater"));
        }
    }

    #[test]
    fn grow_managed_disk_rejects_missing_non_regular_and_non_btrfs_images() {
        let temp = tempdir().unwrap();
        let runner = FakeRunner::default();
        let missing = grow_managed_disk_with_runner(
            temp.path(),
            LibkrunResizeTarget::Nix,
            RAW_NIX_DISK_SPEC.size_bytes + GIB,
            &runner,
        )
        .unwrap_err();
        assert!(missing.to_string().contains("does not exist"));

        fs::create_dir(temp.path().join(RAW_NIX_DISK_SPEC.file_name)).unwrap();
        let dir_err = grow_managed_disk_with_runner(
            temp.path(),
            LibkrunResizeTarget::Nix,
            RAW_NIX_DISK_SPEC.size_bytes + GIB,
            &runner,
        )
        .unwrap_err();
        assert!(dir_err.to_string().contains("not a regular file"));

        let temp = tempdir().unwrap();
        let path = temp.path().join(RAW_NIX_DISK_SPEC.file_name);
        let file = File::create(&path).unwrap();
        file.set_len(RAW_NIX_DISK_SPEC.size_bytes).unwrap();
        drop(file);
        let runner = FakeRunner::with_probe("ext4");
        let fs_err = grow_managed_disk_with_runner(
            temp.path(),
            LibkrunResizeTarget::Nix,
            RAW_NIX_DISK_SPEC.size_bytes + GIB,
            &runner,
        )
        .unwrap_err();
        assert!(fs_err.to_string().contains("expected btrfs"));
    }

    #[test]
    fn live_probe_blocks_matching_krun_disk_path_and_ignores_non_matching() {
        let target = Path::new("/tmp/state/libkrun-nix.raw");
        let runner = FakePodmanOutputRunner::new([
            Ok("abc\ndef\n".to_owned()),
            Ok("krun.disk.0.path=/tmp/state/libkrun-nix.raw\n".to_owned()),
            Ok("krun.disk.1.path=/other.raw\n".to_owned()),
        ]);

        let users = live_raw_disk_users(target, &runner).unwrap();

        assert_eq!(users, ["abc"]);
    }

    #[test]
    fn live_probe_fails_closed_on_podman_errors() {
        let target = Path::new("/tmp/state/libkrun-nix.raw");
        let runner = FakePodmanOutputRunner::new([Err("podman unavailable".to_owned())]);

        let err = ensure_no_live_raw_disk_users(target, &runner).unwrap_err();

        assert!(err.to_string().contains("podman unavailable"));
    }

    #[test]
    fn resize_builder_is_non_interactive_and_direct_to_guest_init() {
        let disk = test_disk(LibkrunResizeTarget::Nix);
        let args = build_libkrun_resize_podman_args(LibkrunResizePodmanSpec {
            image: crate::DEFAULT_IMAGE,
            container_name: "agentbox-resize",
            hostname: "agentbox",
            target: LibkrunResizeTarget::Nix,
            disk: &disk,
            ram_mib: 8192,
            cpu_count: Some(8),
            tsi: false,
            guest_profile: false,
            guest_debug: false,
            guest_init_target: "/nix/store/hash-agentbox/bin/agentbox-guest-init",
            guest_init_override: None,
        })
        .unwrap();
        let joined = args.join("\n");

        assert_eq!(args[0], "run");
        assert!(args.contains(&"--rm".to_owned()));
        assert!(!args.contains(&"-it".to_owned()));
        assert!(joined.contains("--userns\nkeep-id"));
        assert!(joined.contains("--user\n0:0"));
        assert!(joined.contains("--entrypoint\n/nix/store/hash-agentbox/bin/agentbox-guest-init"));
        assert!(joined.contains(&format!(
            "{}\nlibkrun\nresize\n--target\nnix",
            crate::DEFAULT_IMAGE
        )));
        assert!(args.contains(&"krun.disk.0.path=/tmp/state/libkrun-nix.raw".to_owned()));
        assert!(!joined.contains("fish"));
        assert!(!joined.contains("\n-l\n"));
        assert!(!joined.contains("default\nenter"));
    }

    #[test]
    fn resize_builder_preserves_guest_init_override_mount() {
        let disk = test_disk(LibkrunResizeTarget::Containers);
        let override_mount = GuestInitOverrideMount {
            source: PathBuf::from("/tmp/agentbox-guest-init"),
            mount_arg:
                "/tmp/agentbox-guest-init:/nix/store/hash-agentbox/bin/agentbox-guest-init:ro"
                    .to_owned(),
            target: "/nix/store/hash-agentbox/bin/agentbox-guest-init".to_owned(),
        };

        let args = build_libkrun_resize_podman_args(LibkrunResizePodmanSpec {
            image: crate::DEFAULT_IMAGE,
            container_name: "agentbox-resize",
            hostname: "agentbox",
            target: LibkrunResizeTarget::Containers,
            disk: &disk,
            ram_mib: 8192,
            cpu_count: None,
            tsi: true,
            guest_profile: false,
            guest_debug: false,
            guest_init_target: &override_mount.target,
            guest_init_override: Some(&override_mount),
        })
        .unwrap();
        let joined = args.join("\n");

        assert!(joined.contains(&format!("--volume\n{}", override_mount.mount_arg)));
        assert!(joined.contains("--entrypoint\n/nix/store/hash-agentbox/bin/agentbox-guest-init"));
        assert!(joined.contains(&format!(
            "{}\nlibkrun\nresize\n--target\ncontainers",
            crate::DEFAULT_IMAGE
        )));
        assert!(args.contains(&"krun.disk.1.path=/tmp/state/libkrun-containers.raw".to_owned()));
    }

    #[test]
    fn guest_failure_message_calls_out_retry_without_rollback() {
        let message = retry_after_guest_failure_message(
            LibkrunResizeTarget::Nix,
            Path::new("/tmp/state/libkrun-nix.raw"),
            Some(2),
        );

        assert!(message.contains("rerun the same resize command"));
        assert!(message.contains("will not be shrunk or reset"));
        assert!(message.contains("exited with status 2"));
    }

    struct FakePodmanOutputRunner {
        outputs: RefCell<VecDeque<Result<String, String>>>,
    }

    impl FakePodmanOutputRunner {
        fn new<const N: usize>(outputs: [Result<String, String>; N]) -> Self {
            Self {
                outputs: RefCell::new(outputs.into()),
            }
        }
    }

    impl PodmanOutputRunner for FakePodmanOutputRunner {
        fn output(&self, _args: Vec<String>, _context: &str) -> Result<String> {
            match self.outputs.borrow_mut().pop_front() {
                Some(Ok(output)) => Ok(output),
                Some(Err(message)) => Err(anyhow!(message)),
                None => Err(anyhow!("unexpected podman call")),
            }
        }
    }

    fn test_disk(target: LibkrunResizeTarget) -> RawBtrfsDisk {
        let (path, spec) = match target {
            LibkrunResizeTarget::Nix => ("/tmp/state/libkrun-nix.raw", &RAW_NIX_DISK_SPEC),
            LibkrunResizeTarget::Containers => (
                "/tmp/state/libkrun-containers.raw",
                &RAW_CONTAINER_DISK_SPEC,
            ),
        };
        RawBtrfsDisk {
            path: PathBuf::from(path),
            id: spec.id.to_owned(),
            label: spec.label.to_owned(),
            size_bytes: spec.size_bytes,
            status: RawBtrfsDiskStatus::Reused,
        }
    }
}
