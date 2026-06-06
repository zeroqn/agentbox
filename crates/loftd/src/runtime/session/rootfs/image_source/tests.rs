use super::*;
use std::cell::RefCell;

#[derive(Debug)]
struct FakeBuildahCommands {
    local_image_exists: bool,
    output: String,
    calls: RefCell<Vec<Vec<String>>>,
}

impl FakeBuildahCommands {
    fn new(local_image_exists: bool, rootfs_path: &Path) -> Self {
        Self {
            local_image_exists,
            output: format!(
                "selected_image={}\nimage_digest=sha256:feedface\nrootfs_path={}\n",
                DEFAULT_FALLBACK_IMAGE,
                rootfs_path.display()
            ),
            calls: RefCell::new(Vec::new()),
        }
    }

    fn with_output(mut self, output: String) -> Self {
        self.output = output;
        self
    }

    fn calls(&self) -> Vec<Vec<String>> {
        self.calls.borrow().clone()
    }
}

impl BuildahCommands for FakeBuildahCommands {
    fn run(&self, args: &[&str]) -> Result<String> {
        self.calls
            .borrow_mut()
            .push(args.iter().map(|arg| (*arg).to_owned()).collect());
        match args {
            ["--version"] => Ok("buildah version 1.42.0\n".to_owned()),
            other => bail!("unexpected buildah args: {other:?}"),
        }
    }

    fn status(&self, args: &[&str]) -> Result<bool> {
        self.calls
            .borrow_mut()
            .push(args.iter().map(|arg| (*arg).to_owned()).collect());
        match args {
            ["inspect", "--type", "image", DEFAULT_IMAGE] => Ok(self.local_image_exists),
            other => bail!("unexpected buildah status args: {other:?}"),
        }
    }

    fn run_unshare_materializer(&self, args: &[&str]) -> Result<String> {
        self.calls.borrow_mut().push(
            std::iter::once("unshare-materializer".to_owned())
                .chain(args.iter().map(|arg| (*arg).to_owned()))
                .collect(),
        );
        Ok(self.output.clone())
    }
}

fn write_guest_init(root: &Path, store_name: &str, mode: u32) -> PathBuf {
    let path = root
        .join("nix/store")
        .join(store_name)
        .join("bin")
        .join(GUEST_INIT_BASENAME);
    fs::create_dir_all(path.parent().unwrap()).expect("parent should be created");
    fs::write(&path, "#!/bin/sh\n").expect("guest init should be written");
    fs::set_permissions(&path, fs::Permissions::from_mode(mode)).expect("mode should be set");
    path
}

#[derive(Debug)]
struct FakeChildBuildahCommands {
    mount_root: PathBuf,
    calls: RefCell<Vec<Vec<String>>>,
}

impl FakeChildBuildahCommands {
    fn new(mount_root: PathBuf) -> Self {
        Self {
            mount_root,
            calls: RefCell::new(Vec::new()),
        }
    }

    fn calls(&self) -> Vec<Vec<String>> {
        self.calls.borrow().clone()
    }
}

impl ChildBuildahCommands for FakeChildBuildahCommands {
    fn run(&self, args: &[&str]) -> Result<String> {
        self.calls
            .borrow_mut()
            .push(args.iter().map(|arg| (*arg).to_owned()).collect());
        match args {
            ["from", "--pull=missing", "ghcr.io/example/loftd:dev"] => {
                Ok("fake-container\n".to_owned())
            }
            [
                "inspect",
                "--format",
                "{{.FromImageDigest}}",
                "fake-container",
            ] => Ok("sha256:feedface\n".to_owned()),
            [
                "inspect",
                "--format",
                OCI_PROCESS_CONFIG_TEMPLATE,
                "fake-container",
            ] => Ok(
                concat!(
                    "oci_env.0=504154483d2f6e69782f73746f72652f666973682f62696e\n",
                    "oci_cmd.0=66697368\n",
                    "oci_cmd.1=2d6c\n",
                    "oci_entrypoint.0=2f6e69782f73746f72652f686173682d6c6f6674642f62696e2f6c6f6674642d67756573742d696e6974\n",
                    "oci_entrypoint.1=656e746572\n",
                    "oci_entrypoint.2=2d2d\n",
                    "oci_workdir=2f776f726b73706163652f70726f6a656374\n"
                )
                .to_owned(),
            ),
            ["mount", "fake-container"] => Ok(format!("{}\n", self.mount_root.display())),
            ["umount", "fake-container"] => Ok(String::new()),
            ["rm", "fake-container"] => Ok(String::new()),
            other => bail!("unexpected child buildah args: {other:?}"),
        }
    }
}

#[derive(Debug)]
struct FakeBtrfsRootfsCommands {
    calls: RefCell<Vec<(PathBuf, PathBuf)>>,
}

impl FakeBtrfsRootfsCommands {
    fn new() -> Self {
        Self {
            calls: RefCell::new(Vec::new()),
        }
    }
}

impl BtrfsRootfsCommands for FakeBtrfsRootfsCommands {
    fn snapshot_btrfs_subvolume(&self, source: &Path, destination: &Path) -> Result<()> {
        self.calls
            .borrow_mut()
            .push((source.to_path_buf(), destination.to_path_buf()));
        fs::create_dir_all(destination).expect("destination should be created");
        Ok(())
    }

    fn delete_btrfs_subvolume(&self, _subvolume: &Path) -> Result<()> {
        Ok(())
    }
}

#[test]
fn default_selection_uses_localhost_with_pull_never_when_present() {
    let temp = tempfile::tempdir().expect("tempdir should exist");
    let output = format!(
        "selected_image={DEFAULT_IMAGE}\nimage_digest=sha256:local\nrootfs_path={}\n",
        temp.path().join("rootfs").display()
    );
    let commands = FakeBuildahCommands::new(true, &temp.path().join("rootfs")).with_output(output);

    let rootfs = materialize_btrfs_source_rootfs(
        &ImageSelection::PreferLocalhostThenCanonical,
        &temp.path().join("rootfs"),
        &commands,
    )
    .expect("local image should materialize");

    assert_eq!(rootfs.selected_reference, DEFAULT_IMAGE);
    assert_eq!(rootfs.image_digest.as_deref(), Some("sha256:local"));
    assert_eq!(
        commands.calls(),
        vec![
            vec!["--version"],
            vec!["inspect", "--type", "image", DEFAULT_IMAGE],
            vec![
                "unshare-materializer",
                DEFAULT_IMAGE,
                "never",
                temp.path().join("rootfs").to_str().unwrap()
            ],
        ]
    );
}

#[test]
fn default_selection_falls_back_to_canonical_with_pull_missing() {
    let temp = tempfile::tempdir().expect("tempdir should exist");
    let commands = FakeBuildahCommands::new(false, &temp.path().join("rootfs"));

    materialize_btrfs_source_rootfs(
        &ImageSelection::PreferLocalhostThenCanonical,
        &temp.path().join("rootfs"),
        &commands,
    )
    .expect("canonical fallback should materialize");

    assert_eq!(
        commands.calls(),
        vec![
            vec!["--version"],
            vec!["inspect", "--type", "image", DEFAULT_IMAGE],
            vec![
                "unshare-materializer",
                DEFAULT_FALLBACK_IMAGE,
                "missing",
                temp.path().join("rootfs").to_str().unwrap()
            ],
        ]
    );
}

#[test]
fn pull_latest_uses_canonical_with_pull_always() {
    let temp = tempfile::tempdir().expect("tempdir should exist");
    let commands = FakeBuildahCommands::new(false, &temp.path().join("rootfs"));

    materialize_btrfs_source_rootfs(
        &ImageSelection::CanonicalWithRefresh,
        &temp.path().join("rootfs"),
        &commands,
    )
    .expect("canonical refresh should materialize");

    assert_eq!(
        commands.calls(),
        vec![
            vec!["--version"],
            vec![
                "unshare-materializer",
                DEFAULT_FALLBACK_IMAGE,
                "always",
                temp.path().join("rootfs").to_str().unwrap()
            ],
        ]
    );
}

#[test]
fn explicit_image_uses_pull_missing_without_refs_file() {
    let temp = tempfile::tempdir().expect("tempdir should exist");
    let commands = FakeBuildahCommands::new(false, &temp.path().join("rootfs"));
    let selection = ImageSelection::Explicit {
        reference: "ghcr.io/example/loftd@sha256:abc123".to_owned(),
    };

    materialize_btrfs_source_rootfs(&selection, &temp.path().join("rootfs"), &commands)
        .expect("explicit image should materialize");

    assert_eq!(
        commands.calls(),
        vec![
            vec!["--version"],
            vec![
                "unshare-materializer",
                "ghcr.io/example/loftd@sha256:abc123",
                "missing",
                temp.path().join("rootfs").to_str().unwrap()
            ],
        ]
    );
}

#[test]
fn materializer_output_accepts_missing_digest() {
    let rootfs = parse_materializer_output(
        "selected_image=localhost/loftd:latest\nimage_digest=<no value>\nrootfs_path=/tmp/rootfs\n",
    )
    .expect("output should parse");

    assert_eq!(rootfs.selected_reference, "localhost/loftd:latest");
    assert_eq!(rootfs.image_digest, None);
    assert_eq!(rootfs.rootfs_path, Path::new("/tmp/rootfs"));
    assert_eq!(rootfs.process_config, OciProcessConfig::default());
}

#[test]
fn materializer_output_parses_oci_process_config_fields() {
    let rootfs = parse_materializer_output(
        concat!(
            "selected_image=localhost/loftd:latest\n",
            "image_digest=sha256:feedface\n",
            "rootfs_path=/tmp/rootfs\n",
            "oci_env.1=484f4d453d2f686f6d652f646576\n",
            "oci_env.0=504154483d2f6e69782f73746f72652f666973682f62696e\n",
            "oci_cmd.0=66697368\n",
            "oci_cmd.1=2d6c\n",
            "oci_entrypoint.0=2f6e69782f73746f72652f686173682d6c6f6674642f62696e2f6c6f6674642d67756573742d696e6974\n",
            "oci_entrypoint.1=656e746572\n",
            "oci_entrypoint.2=2d2d\n",
            "oci_workdir=2f776f726b7370616365\n",
        ),
    )
    .expect("metadata output should parse");

    assert_eq!(
        rootfs.process_config,
        OciProcessConfig {
            env: vec![
                "PATH=/nix/store/fish/bin".to_owned(),
                "HOME=/home/dev".to_owned()
            ],
            cmd: vec!["fish".to_owned(), "-l".to_owned()],
            entrypoint: vec![
                "/nix/store/hash-loftd/bin/loftd-guest-init".to_owned(),
                "enter".to_owned(),
                "--".to_owned()
            ],
            working_dir: Some("/workspace".to_owned()),
        }
    );
}

#[test]
fn materializer_output_rejects_malformed_known_oci_fields() {
    assert!(
        parse_materializer_output(
            "selected_image=localhost/loftd:latest\nrootfs_path=/tmp/rootfs\noci_env.x=61\n",
        )
        .is_err()
    );
    assert!(
        parse_materializer_output(
            "selected_image=localhost/loftd:latest\nrootfs_path=/tmp/rootfs\noci_cmd.0=6\n",
        )
        .is_err()
    );
    assert!(
        parse_materializer_output(
            "selected_image=localhost/loftd:latest\nrootfs_path=/tmp/rootfs\noci_workdir=2f\noci_workdir=2f\n",
        )
        .is_err()
    );
}

#[test]
fn compatibility_check_requires_exactly_one_executable_loftd_guest_init() {
    let temp = tempfile::tempdir().expect("tempdir should exist");
    let guest_init = write_guest_init(temp.path(), "hash-loftd", 0o755);

    assert_eq!(
        find_loftd_guest_init(temp.path()).expect("guest init should resolve"),
        guest_init
    );

    let missing = tempfile::tempdir().expect("tempdir should exist");
    assert!(find_loftd_guest_init(missing.path()).is_err());

    let non_executable = tempfile::tempdir().expect("tempdir should exist");
    write_guest_init(non_executable.path(), "hash-loftd", 0o644);
    assert!(find_loftd_guest_init(non_executable.path()).is_err());

    write_guest_init(temp.path(), "hash-loftd-two", 0o755);
    let err = find_loftd_guest_init(temp.path()).expect_err("multiple init binaries fail");
    assert!(err.to_string().contains("ambiguous"));
}

#[test]
fn internal_child_snapshots_buildah_mount_and_cleans_container_on_success() {
    let temp = tempfile::tempdir().expect("tempdir should exist");
    let mount_root = temp.path().join("mount");
    write_guest_init(&mount_root, "hash-loftd", 0o755);
    let destination = temp.path().join("task-rootfs");
    let buildah = FakeChildBuildahCommands::new(mount_root.clone());
    let btrfs = FakeBtrfsRootfsCommands::new();

    run_btrfs_rootfs_child_with_commands(
        "ghcr.io/example/loftd:dev",
        "missing",
        &destination,
        &buildah,
        &btrfs,
    )
    .expect("child materialization should succeed");

    assert_eq!(
        btrfs.calls.borrow().as_slice(),
        [(mount_root, destination.clone())]
    );
    assert_eq!(
        buildah.calls(),
        vec![
            vec!["from", "--pull=missing", "ghcr.io/example/loftd:dev"],
            vec![
                "inspect",
                "--format",
                "{{.FromImageDigest}}",
                "fake-container"
            ],
            vec![
                "inspect",
                "--format",
                OCI_PROCESS_CONFIG_TEMPLATE,
                "fake-container"
            ],
            vec!["mount", "fake-container"],
            vec!["umount", "fake-container"],
            vec!["rm", "fake-container"],
        ]
    );
}

#[test]
fn internal_child_cleans_container_on_compatibility_failure() {
    let temp = tempfile::tempdir().expect("tempdir should exist");
    let buildah = FakeChildBuildahCommands::new(temp.path().join("mount"));
    let btrfs = FakeBtrfsRootfsCommands::new();

    let error = run_btrfs_rootfs_child_with_commands(
        "ghcr.io/example/loftd:dev",
        "missing",
        &temp.path().join("task-rootfs"),
        &buildah,
        &btrfs,
    )
    .expect_err("missing guest init should fail");

    assert!(format!("{error:#}").contains("not loftd-compatible"));
    assert_eq!(
        buildah.calls(),
        vec![
            vec!["from", "--pull=missing", "ghcr.io/example/loftd:dev"],
            vec![
                "inspect",
                "--format",
                "{{.FromImageDigest}}",
                "fake-container"
            ],
            vec![
                "inspect",
                "--format",
                OCI_PROCESS_CONFIG_TEMPLATE,
                "fake-container"
            ],
            vec!["mount", "fake-container"],
            vec!["umount", "fake-container"],
            vec!["rm", "fake-container"],
        ]
    );
}
