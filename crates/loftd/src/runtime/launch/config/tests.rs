use super::*;
use crate::logging::LogLevel;
use crate::runtime::seccomp::{AuditMode, SeccompMode};
use crate::runtime::session::rootfs::image_source::OciProcessConfig;
use crate::runtime::vm::gpu::GpuMode;
use std::fs;
use std::path::Path;
use tempfile::tempdir;

fn test_mounts() -> Vec<BindMount> {
    vec![
        BindMount::directory("/workspace-src", WORKSPACE_TAG, WORKSPACE_TARGET),
        BindMount::directory("/home/host/.codex", CODEX_TAG, CODEX_TARGET),
        BindMount::directory("/home/host/.omp", OMP_TAG, OMP_TARGET),
        BindMount::directory("/home/host/.pi", PI_TAG, PI_TARGET),
        BindMount::directory(
            "/home/host/.config/dirge",
            DIRGE_CONFIG_TAG,
            DIRGE_CONFIG_TARGET,
        ),
        BindMount::directory(
            "/home/host/.local/share/dirge",
            DIRGE_DATA_TAG,
            DIRGE_DATA_TARGET,
        ),
        BindMount::directory("/home/host/.dirge", DIRGE_HOME_TAG, DIRGE_HOME_TARGET),
        BindMount::directory("/state/project/cargo", CARGO_TAG, CARGO_TARGET),
        BindMount::directory("/state/sccache", SCCACHE_TAG, SCCACHE_TARGET),
    ]
}

fn replace_field_value(text: &mut String, key: &str, value: &str) {
    let mut replacement = String::new();
    push_field(&mut replacement, key, value);
    *text = text
        .lines()
        .map(|line| {
            if line.starts_with(&format!("{key}=")) {
                replacement.trim_end()
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    text.push('\n');
}

#[test]
fn launch_config_defaults_to_guest_init_enter_fish_shell() {
    let image_process_config = OciProcessConfig::default();
    let config = LaunchConfig::build_for_task(LaunchSpec {
        task_rootfs: Path::new("/state/task/rootfs"),
        hostname: "loftd-workspace",
        mounts: &test_mounts(),
        guest_init_override: None,
        guest_init_exec: "/nix/store/hash-loftd/bin/loftd-guest-init",
        guest_command: &[],
        image_process_config: &image_process_config,
        mem_gib: Some(4),
        log_level: LogLevel::Debug,
        network_mode: NetworkMode::Tsi,
        gpu_mode: GpuMode::Off,
        wayland: false,
        io_uring: true,
        perf: true,
        publish: &[],
        profile: true,
        root: false,
        allocator: AllocatorMode::Mimalloc,
        host_uid: 1000,
        host_gid: 1001,
        vcpus: 2,
        disks: Vec::new(),
        extra_env: Vec::new(),
        host_nix_overlay: None,
        waypipe: None,
        managed_session: None,
    })
    .expect("launch config should build");

    assert_eq!(config.task_rootfs, Path::new("/state/task/rootfs"));
    assert_eq!(config.mounts[0].source, Path::new("/workspace-src"));
    assert_eq!(config.mounts[0].tag, "loftd-workspace");
    assert_eq!(config.mounts[0].target, "/workspace");
    assert_eq!(config.mounts.len(), 9);
    assert_eq!(config.ram_mib, 4096);
    assert_eq!(config.vcpus, 2);
    assert_eq!(config.log_level, LogLevel::Debug);
    assert_eq!(config.workdir, "/workspace");
    assert_eq!(
        config.exec_path,
        "/nix/store/hash-loftd/bin/loftd-guest-init"
    );
    assert_eq!(config.argv, ["enter", "fish", "-l"]);
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
    assert!(config.guest_config_env_contains("LOFTD_IO_URING", "1"));
    assert!(config.guest_config_env_contains("LOFTD_PERF", "1"));
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
        hostname: "loftd-workspace",
        mounts: &test_mounts(),
        guest_init_override: None,
        guest_init_exec: "/nix/store/hash-loftd/bin/loftd-guest-init",
        guest_command: &command,
        image_process_config: &image_process_config,
        mem_gib: Some(4),
        log_level: LogLevel::Off,
        network_mode: NetworkMode::Tsi,
        gpu_mode: GpuMode::Off,
        wayland: false,
        io_uring: false,
        perf: false,
        publish: &[],
        profile: false,
        root: false,
        allocator: AllocatorMode::Mimalloc,
        host_uid: 1000,
        host_gid: 1001,
        vcpus: 2,
        disks: Vec::new(),
        extra_env: Vec::new(),
        host_nix_overlay: None,
        waypipe: None,
        managed_session: None,
    })
    .expect("launch config should build");

    assert_eq!(config.argv, ["enter", "bash", "-lc", "echo ok"]);
    assert_ne!(config.argv.get(1).map(String::as_str), Some("--"));
    assert!(
        config
            .guest_config_env
            .iter()
            .all(|(key, _)| key != "LOFTD_IO_URING" && key != "LOFTD_PERF")
    );
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
        hostname: "loftd-workspace",
        mounts: &test_mounts(),
        guest_init_override: Some(GuestInitOverrideMount {
            source: Path::new("/host/override-loftd-guest-init").to_path_buf(),
            target: "/nix/store/hash-loftd/bin/loftd-guest-init".to_owned(),
            read_only: true,
        }),
        guest_init_exec: "/nix/store/hash-loftd/bin/loftd-guest-init",
        guest_command: &[],
        image_process_config: &image_process_config,
        mem_gib: Some(2),
        log_level: LogLevel::Off,
        network_mode: NetworkMode::Tsi,
        gpu_mode: GpuMode::Off,
        wayland: false,
        io_uring: false,
        perf: false,
        publish: &[],
        profile: false,
        root: true,
        allocator: AllocatorMode::Mimalloc,
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
        host_nix_overlay: None,
        waypipe: None,
        managed_session: None,
    })
    .expect("launch config should build");

    let parsed = LaunchConfig::parse(&config.serialize()).expect("config should parse");

    assert_eq!(parsed, config);
    assert_eq!(parsed.mounts, test_mounts());
    assert_eq!(
        parsed.guest_init_override,
        Some(GuestInitOverrideMount {
            source: Path::new("/host/override-loftd-guest-init").to_path_buf(),
            target: "/nix/store/hash-loftd/bin/loftd-guest-init".to_owned(),
            read_only: true,
        })
    );
    assert_eq!(
        parsed.env,
        [("KRUN_CONFIG".to_owned(), "/.loftd_config.json".to_owned())]
    );
    assert!(parsed.guest_config_env_contains("LOFTD_ENTER_AS_ROOT", "1"));
    assert!(parsed.guest_config_env_contains("LOFTD_CONTAINERS_STORAGE", "1"));
    assert!(parsed.guest_config_env_contains("PATH", "/nix/store/fish/bin"));
    assert_eq!(parsed.argv, ["enter", "fish", "-l"]);
    assert_eq!(parsed.workdir, "/workspace/project");
    assert_eq!(parsed.disks[0].id, "loftd-nix");
    assert_eq!(
        parsed.disks[1].path,
        Path::new("/state/loftd-containers.raw")
    );
}

#[test]
fn launch_config_round_trips_managed_session_contract() {
    let image_process_config = OciProcessConfig::default();
    let config = LaunchConfig::build_for_task(LaunchSpec {
        task_rootfs: Path::new("/state/task/rootfs"),
        hostname: "loftd-workspace",
        mounts: &test_mounts(),
        guest_init_override: None,
        guest_init_exec: "/nix/store/hash-loftd/bin/loftd-guest-init",
        guest_command: &[],
        image_process_config: &image_process_config,
        mem_gib: Some(4),
        log_level: LogLevel::Off,
        network_mode: NetworkMode::Tsi,
        gpu_mode: GpuMode::Off,
        wayland: false,
        io_uring: false,
        perf: false,
        publish: &[],
        profile: false,
        root: false,
        allocator: AllocatorMode::Mimalloc,
        host_uid: 1000,
        host_gid: 1001,
        vcpus: 2,
        disks: Vec::new(),
        extra_env: Vec::new(),
        host_nix_overlay: None,
        waypipe: Some(WaypipeConfig {
            socket: Path::new("/tmp/loftd-waypipe.sock").to_path_buf(),
            guest_port: 50_427,
        }),
        managed_session: Some(ManagedSessionConfig {
            attach_socket: Path::new("/state/task/attach.sock").to_path_buf(),
            guest_port: 50_426,
            protocol_version: 1,
            attach_socket_uid: 1000,
            attach_socket_gid: 1001,
            cleanup_task_rootfs_on_exit: true,
        }),
    })
    .expect("launch config should build");

    let parsed = LaunchConfig::parse(&config.serialize()).expect("config should parse");

    assert_eq!(parsed.managed_session, config.managed_session);
    assert_eq!(parsed.waypipe, config.waypipe);
    assert!(parsed.guest_config_env_contains("LOFTD_WAYPIPE_PORT", "50427"));
    assert!(parsed.guest_config_env_contains("LOFTD_SESSION_MANAGED", "1"));
    assert!(parsed.guest_config_env_contains("LOFTD_ATTACH_PORT", "50426"));
    assert!(parsed.guest_config_env_contains("LOFTD_ATTACH_PROTOCOL_VERSION", "1"));
}

#[test]
fn managed_session_extra_env_terminal_vars_are_guest_visible() {
    let image_process_config = OciProcessConfig::default();
    let config = LaunchConfig::build_for_task(LaunchSpec {
        task_rootfs: Path::new("/state/task/rootfs"),
        hostname: "loftd-workspace",
        mounts: &test_mounts(),
        guest_init_override: None,
        guest_init_exec: "/nix/store/hash-loftd/bin/loftd-guest-init",
        guest_command: &[],
        image_process_config: &image_process_config,
        mem_gib: Some(4),
        log_level: LogLevel::Off,
        network_mode: NetworkMode::Tsi,
        gpu_mode: GpuMode::Off,
        wayland: false,
        io_uring: false,
        perf: false,
        publish: &[],
        profile: false,
        root: false,
        allocator: AllocatorMode::Mimalloc,
        host_uid: 1000,
        host_gid: 1001,
        vcpus: 2,
        disks: Vec::new(),
        extra_env: vec![
            ("TERM".to_owned(), "xterm-kitty".to_owned()),
            ("COLORTERM".to_owned(), "truecolor".to_owned()),
            ("TERM_PROGRAM".to_owned(), "ghostty".to_owned()),
            ("TERM_PROGRAM_VERSION".to_owned(), "1.2.3".to_owned()),
        ],
        host_nix_overlay: None,
        waypipe: None,
        managed_session: Some(ManagedSessionConfig {
            attach_socket: Path::new("/state/task/attach.sock").to_path_buf(),
            guest_port: 50_426,
            protocol_version: 1,
            attach_socket_uid: 1000,
            attach_socket_gid: 1001,
            cleanup_task_rootfs_on_exit: true,
        }),
    })
    .expect("launch config should build");

    assert!(config.guest_config_env_contains("TERM", "xterm-kitty"));
    assert!(config.guest_config_env_contains("COLORTERM", "truecolor"));
    assert!(config.guest_config_env_contains("TERM_PROGRAM", "ghostty"));
    assert!(config.guest_config_env_contains("TERM_PROGRAM_VERSION", "1.2.3"));
}

#[test]
fn non_managed_launch_does_not_allow_terminal_identity_from_image_env() {
    let image_process_config = OciProcessConfig {
        env: vec![
            "PATH=/bin".to_owned(),
            "TERM=xterm-kitty".to_owned(),
            "COLORTERM=truecolor".to_owned(),
            "TERM_PROGRAM=ghostty".to_owned(),
            "TERM_PROGRAM_VERSION=1.2.3".to_owned(),
            "TERM_PROGGRAM_VERSION=typo".to_owned(),
        ],
        ..OciProcessConfig::default()
    };
    let config = LaunchConfig::build_for_task(LaunchSpec {
        task_rootfs: Path::new("/state/task/rootfs"),
        hostname: "loftd-workspace",
        mounts: &test_mounts(),
        guest_init_override: None,
        guest_init_exec: "/nix/store/hash-loftd/bin/loftd-guest-init",
        guest_command: &[],
        image_process_config: &image_process_config,
        mem_gib: Some(4),
        log_level: LogLevel::Off,
        network_mode: NetworkMode::Tsi,
        gpu_mode: GpuMode::Off,
        wayland: false,
        io_uring: false,
        perf: false,
        publish: &[],
        profile: false,
        root: false,
        allocator: AllocatorMode::Mimalloc,
        host_uid: 1000,
        host_gid: 1001,
        vcpus: 2,
        disks: Vec::new(),
        extra_env: Vec::new(),
        host_nix_overlay: None,
        waypipe: None,
        managed_session: None,
    })
    .expect("launch config should build");

    assert!(config.guest_config_env_contains("PATH", "/bin"));
    assert!(config.guest_config_env.iter().all(|(key, _)| !matches!(
        key.as_str(),
        "TERM" | "COLORTERM" | "TERM_PROGRAM" | "TERM_PROGRAM_VERSION" | "TERM_PROGGRAM_VERSION"
    )));
}

#[test]
fn launch_config_round_trips_seccomp_modes() {
    let mut config = LaunchConfig::build_for_task(LaunchSpec {
        task_rootfs: Path::new("/state/task/rootfs"),
        hostname: "loftd-workspace",
        mounts: &test_mounts(),
        guest_init_override: None,
        guest_init_exec: "/nix/store/hash-loftd/bin/loftd-guest-init",
        guest_command: &[],
        image_process_config: &OciProcessConfig::default(),
        mem_gib: Some(4),
        log_level: LogLevel::Off,
        network_mode: NetworkMode::Tsi,
        gpu_mode: GpuMode::Off,
        wayland: false,
        io_uring: false,
        perf: false,
        publish: &[],
        profile: false,
        root: false,
        allocator: AllocatorMode::Mimalloc,
        host_uid: 1000,
        host_gid: 1001,
        vcpus: 2,
        disks: Vec::new(),
        extra_env: Vec::new(),
        host_nix_overlay: None,
        waypipe: None,
        managed_session: None,
    })
    .expect("launch config should build");
    config.seccomp = SeccompMode::Audit(AuditMode::Full {
        trace_path: Path::new("/tmp/loftd.trace.jsonl").to_path_buf(),
    });

    let parsed = LaunchConfig::parse(&config.serialize()).expect("audit config should parse");
    assert_eq!(parsed.seccomp, config.seccomp);

    config.seccomp = SeccompMode::Audit(AuditMode::Gap {
        baseline_policy_path: Path::new("/tmp/loftd.baseline.json").to_path_buf(),
        trace_path: Path::new("/tmp/loftd.denied.jsonl").to_path_buf(),
    });

    let parsed = LaunchConfig::parse(&config.serialize()).expect("gap audit config should parse");
    assert_eq!(parsed.seccomp, config.seccomp);

    config.seccomp = SeccompMode::Enforce {
        policy_path: Path::new("/tmp/loftd.policy.json").to_path_buf(),
    };

    let parsed = LaunchConfig::parse(&config.serialize()).expect("enforce config should parse");
    assert_eq!(parsed.seccomp, config.seccomp);
}

#[test]
fn launch_config_round_trips_landlock_modes() {
    let mut config = LaunchConfig::build_for_task(LaunchSpec {
        task_rootfs: Path::new("/state/task/rootfs"),
        hostname: "loftd-workspace",
        mounts: &test_mounts(),
        guest_init_override: None,
        guest_init_exec: "/nix/store/hash-loftd/bin/loftd-guest-init",
        guest_command: &[],
        image_process_config: &OciProcessConfig::default(),
        mem_gib: Some(4),
        log_level: LogLevel::Off,
        network_mode: NetworkMode::Tsi,
        gpu_mode: GpuMode::Off,
        wayland: false,
        io_uring: false,
        perf: false,
        publish: &[],
        profile: false,
        root: false,
        allocator: AllocatorMode::Mimalloc,
        host_uid: 1000,
        host_gid: 1001,
        vcpus: 2,
        disks: Vec::new(),
        extra_env: Vec::new(),
        host_nix_overlay: None,
        waypipe: None,
        managed_session: None,
    })
    .expect("launch config should build");

    for mode in [
        crate::runtime::landlock::LandlockMode::All,
        crate::runtime::landlock::LandlockMode::Relax,
        crate::runtime::landlock::LandlockMode::BestEffort,
        crate::runtime::landlock::LandlockMode::Off,
    ] {
        config.landlock = mode;
        let parsed = LaunchConfig::parse(&config.serialize()).expect("config should parse");
        assert_eq!(parsed.landlock, mode);
        assert_eq!(parsed.perf, config.perf);
        assert_eq!(parsed, config);
    }
}

#[test]
fn launch_config_legacy_missing_landlock_mode_defaults_to_relax() {
    let mut config = LaunchConfig::build_for_task(LaunchSpec {
        task_rootfs: Path::new("/state/task/rootfs"),
        hostname: "loftd-workspace",
        mounts: &test_mounts(),
        guest_init_override: None,
        guest_init_exec: "/nix/store/hash-loftd/bin/loftd-guest-init",
        guest_command: &[],
        image_process_config: &OciProcessConfig::default(),
        mem_gib: Some(4),
        log_level: LogLevel::Off,
        network_mode: NetworkMode::Tsi,
        gpu_mode: GpuMode::Off,
        wayland: false,
        io_uring: false,
        perf: false,
        publish: &[],
        profile: false,
        root: false,
        allocator: AllocatorMode::Mimalloc,
        host_uid: 1000,
        host_gid: 1001,
        vcpus: 2,
        disks: Vec::new(),
        extra_env: Vec::new(),
        host_nix_overlay: None,
        waypipe: None,
        managed_session: None,
    })
    .expect("launch config should build")
    .serialize();
    config = config
        .lines()
        .filter(|line| !line.starts_with("landlock.mode="))
        .collect::<Vec<_>>()
        .join("\n");
    config.push('\n');

    let parsed = LaunchConfig::parse(&config).expect("legacy config should parse");

    assert_eq!(
        parsed.landlock,
        crate::runtime::landlock::LandlockMode::Relax
    );
}

#[test]
fn launch_config_legacy_missing_perf_defaults_to_false() {
    let config = LaunchConfig::build_for_task(LaunchSpec {
        task_rootfs: Path::new("/state/task/rootfs"),
        hostname: "loftd-workspace",
        mounts: &test_mounts(),
        guest_init_override: None,
        guest_init_exec: "/nix/store/hash-loftd/bin/loftd-guest-init",
        guest_command: &[],
        image_process_config: &OciProcessConfig::default(),
        mem_gib: Some(4),
        log_level: LogLevel::Off,
        network_mode: NetworkMode::Tsi,
        gpu_mode: GpuMode::Off,
        wayland: false,
        io_uring: false,
        perf: true,
        publish: &[],
        profile: false,
        root: false,
        allocator: AllocatorMode::Mimalloc,
        host_uid: 1000,
        host_gid: 1001,
        vcpus: 2,
        disks: Vec::new(),
        extra_env: Vec::new(),
        host_nix_overlay: None,
        waypipe: None,
        managed_session: None,
    })
    .expect("launch config should build")
    .serialize()
    .lines()
    .filter(|line| !line.starts_with("perf="))
    .collect::<Vec<_>>()
    .join("\n");

    let parsed = LaunchConfig::parse(&format!("{config}\n")).expect("legacy config should parse");

    assert!(!parsed.perf);
}

#[test]
fn launch_config_rejects_legacy_enforce_landlock_mode() {
    let mut config = LaunchConfig::build_for_task(LaunchSpec {
        task_rootfs: Path::new("/state/task/rootfs"),
        hostname: "loftd-workspace",
        mounts: &test_mounts(),
        guest_init_override: None,
        guest_init_exec: "/nix/store/hash-loftd/bin/loftd-guest-init",
        guest_command: &[],
        image_process_config: &OciProcessConfig::default(),
        mem_gib: Some(4),
        log_level: LogLevel::Off,
        network_mode: NetworkMode::Tsi,
        gpu_mode: GpuMode::Off,
        wayland: false,
        io_uring: false,
        perf: false,
        publish: &[],
        profile: false,
        root: false,
        allocator: AllocatorMode::Mimalloc,
        host_uid: 1000,
        host_gid: 1001,
        vcpus: 2,
        disks: Vec::new(),
        extra_env: Vec::new(),
        host_nix_overlay: None,
        waypipe: None,
        managed_session: None,
    })
    .expect("launch config should build")
    .serialize();
    replace_field_value(&mut config, "landlock.mode", "enforce");

    let err = LaunchConfig::parse(&config).expect_err("legacy enforce should be rejected");

    assert!(format!("{err:#}").contains("landlock.mode is invalid"));
}

#[test]
fn launch_config_refuses_to_serialize_unresolved_default_gap_audit() {
    let image_process_config = OciProcessConfig::default();
    let mut config = LaunchConfig::build_for_task(LaunchSpec {
        task_rootfs: Path::new("/state/task/rootfs"),
        hostname: "loftd-workspace",
        mounts: &test_mounts(),
        guest_init_override: None,
        guest_init_exec: "/nix/store/hash-loftd/bin/loftd-guest-init",
        guest_command: &[],
        image_process_config: &image_process_config,
        mem_gib: Some(4),
        log_level: LogLevel::Off,
        network_mode: NetworkMode::Tsi,
        gpu_mode: GpuMode::Off,
        wayland: false,
        io_uring: false,
        perf: false,
        publish: &[],
        profile: false,
        root: false,
        allocator: AllocatorMode::Mimalloc,
        host_uid: 1000,
        host_gid: 1001,
        vcpus: 2,
        disks: Vec::new(),
        extra_env: Vec::new(),
        host_nix_overlay: None,
        waypipe: None,
        managed_session: None,
    })
    .expect("launch config should build");
    config.seccomp = SeccompMode::Audit(AuditMode::DefaultGap {
        trace_path: Path::new("/tmp/loftd.denied.jsonl").to_path_buf(),
    });

    let panic = std::panic::catch_unwind(|| config.serialize())
        .expect_err("unresolved default gap audit should not serialize");
    let message = panic
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| panic.downcast_ref::<String>().map(String::as_str))
        .unwrap_or_default();
    assert!(message.contains("unresolved default seccomp gap audit"));
}

#[test]
fn launch_config_rejects_inconsistent_seccomp_fields() {
    let config = LaunchConfig::build_for_task(LaunchSpec {
        task_rootfs: Path::new("/state/task/rootfs"),
        hostname: "loftd-workspace",
        mounts: &test_mounts(),
        guest_init_override: None,
        guest_init_exec: "/nix/store/hash-loftd/bin/loftd-guest-init",
        guest_command: &[],
        image_process_config: &OciProcessConfig::default(),
        mem_gib: Some(4),
        log_level: LogLevel::Off,
        network_mode: NetworkMode::Tsi,
        gpu_mode: GpuMode::Off,
        wayland: false,
        io_uring: false,
        perf: false,
        publish: &[],
        profile: false,
        root: false,
        allocator: AllocatorMode::Mimalloc,
        host_uid: 1000,
        host_gid: 1001,
        vcpus: 2,
        disks: Vec::new(),
        extra_env: Vec::new(),
        host_nix_overlay: None,
        waypipe: None,
        managed_session: None,
    })
    .expect("launch config should build");

    let mut off_with_audit_path = config.serialize();
    push_field(
        &mut off_with_audit_path,
        "seccomp.audit_trace_path",
        "/tmp/trace.jsonl",
    );
    let err =
        LaunchConfig::parse(&off_with_audit_path).expect_err("off mode should reject audit path");
    assert!(format!("{err:#}").contains("off mode rejects seccomp path fields"));

    let mut audit_config = config.clone();
    audit_config.seccomp = SeccompMode::Audit(AuditMode::Full {
        trace_path: Path::new("/tmp/trace.jsonl").to_path_buf(),
    });
    let mut audit_with_enforce_path = audit_config.serialize();
    push_field(
        &mut audit_with_enforce_path,
        "seccomp.enforce_policy_path",
        "/tmp/policy.json",
    );
    let err = LaunchConfig::parse(&audit_with_enforce_path)
        .expect_err("audit mode should reject enforce path");
    assert!(format!("{err:#}").contains("audit mode rejects seccomp.enforce_policy_path"));

    let mut enforce_config = config;
    enforce_config.seccomp = SeccompMode::Enforce {
        policy_path: Path::new("/tmp/policy.json").to_path_buf(),
    };
    let mut enforce_with_audit_path = enforce_config.serialize();
    push_field(
        &mut enforce_with_audit_path,
        "seccomp.audit_baseline_policy_path",
        "/tmp/baseline.json",
    );
    let err = LaunchConfig::parse(&enforce_with_audit_path)
        .expect_err("enforce mode should reject audit path");
    assert!(format!("{err:#}").contains("enforce mode rejects seccomp audit path fields"));
}

#[test]
fn launch_config_round_trips_volume_source_kind_and_access_mode() {
    let mut mounts = test_mounts();
    let user_volume_index = mounts.len();
    let user_volume_tag = format!("loftd-user-volume-{user_volume_index}");
    mounts.push(BindMount::file(
        "/host/config.json",
        &user_volume_tag,
        "/workspace/config.json",
        true,
    ));
    let image_process_config = OciProcessConfig::default();
    let config = LaunchConfig::build_for_task(LaunchSpec {
        task_rootfs: Path::new("/state/task/rootfs"),
        hostname: "loftd-workspace",
        mounts: &mounts,
        guest_init_override: None,
        guest_init_exec: "/nix/store/hash-loftd/bin/loftd-guest-init",
        guest_command: &[],
        image_process_config: &image_process_config,
        mem_gib: Some(4),
        log_level: LogLevel::Off,
        network_mode: NetworkMode::Tsi,
        gpu_mode: GpuMode::Off,
        wayland: false,
        io_uring: false,
        perf: false,
        publish: &[],
        profile: false,
        root: false,
        allocator: AllocatorMode::Mimalloc,
        host_uid: 1000,
        host_gid: 1001,
        vcpus: 2,
        disks: Vec::new(),
        extra_env: Vec::new(),
        host_nix_overlay: None,
        waypipe: None,
        managed_session: None,
    })
    .expect("launch config should build");

    let decoded = decode_text_for_debug(&config.serialize()).expect("debug decode");
    assert!(decoded.contains(&format!("mount.{user_volume_index}.source_kind=file\n")));
    assert!(decoded.contains(&format!("mount.{user_volume_index}.read_only=true\n")));

    let parsed = LaunchConfig::parse(&config.serialize()).expect("config should parse");

    assert_eq!(parsed, config);
    assert_eq!(
        parsed.mounts[user_volume_index].source_kind,
        BindMountSourceKind::File
    );
    assert!(parsed.mounts[user_volume_index].read_only);
}

#[test]
fn launch_config_carries_host_nix_overlay_and_adds_reserved_nix_mount() {
    let image_process_config = OciProcessConfig::default();
    let overlay = HostNixOverlay {
        selected_reference: "localhost/loftd:latest".to_owned(),
        image_digest: "sha256:deadbeef".to_owned(),
        digest_key: "sha256-deadbeef".to_owned(),
        lowerdir: Path::new("/cache/btrfs-snapshots/sha256-deadbeef/rootfs/nix").to_path_buf(),
        upperdir: Path::new("/state/workspace/nix-overlay/upper").to_path_buf(),
        workdir: Path::new("/state/workspace/nix-overlay/work").to_path_buf(),
        mergeddir: Path::new("/state/workspace/nix-overlay/merged").to_path_buf(),
    };
    let config = LaunchConfig::build_for_task(LaunchSpec {
        task_rootfs: Path::new("/state/task/rootfs"),
        hostname: "loftd-workspace",
        mounts: &test_mounts(),
        guest_init_override: None,
        guest_init_exec: "/nix/store/hash-loftd/bin/loftd-guest-init",
        guest_command: &[],
        image_process_config: &image_process_config,
        mem_gib: Some(4),
        log_level: LogLevel::Off,
        network_mode: NetworkMode::Tsi,
        gpu_mode: GpuMode::Off,
        wayland: false,
        io_uring: false,
        perf: false,
        publish: &[],
        profile: false,
        root: false,
        allocator: AllocatorMode::Mimalloc,
        host_uid: 1000,
        host_gid: 1001,
        vcpus: 2,
        disks: Vec::new(),
        extra_env: Vec::new(),
        host_nix_overlay: Some(overlay.clone()),
        waypipe: None,
        managed_session: None,
    })
    .expect("host nix overlay config should build");

    let nix_mount = config
        .mounts
        .iter()
        .find(|mount| mount.target == "/nix")
        .expect("/nix mount should be added");
    assert_eq!(nix_mount.source, overlay.mergeddir);
    assert_eq!(nix_mount.tag, "loftd-nix");
    assert_eq!(config.host_nix_overlay, Some(overlay.clone()));

    let parsed = LaunchConfig::parse(&config.serialize()).expect("config should parse");

    assert_eq!(parsed.host_nix_overlay, Some(overlay));
    assert_eq!(parsed, config);
}

#[test]
fn launch_config_rejects_user_mount_that_collides_with_host_nix_overlay() {
    let image_process_config = OciProcessConfig::default();
    let mut mounts = test_mounts();
    mounts.push(BindMount::directory("/host/nix", "user-nix", "/nix"));
    let err = LaunchConfig::build_for_task(LaunchSpec {
        task_rootfs: Path::new("/state/task/rootfs"),
        hostname: "loftd-workspace",
        mounts: &mounts,
        guest_init_override: None,
        guest_init_exec: "/nix/store/hash-loftd/bin/loftd-guest-init",
        guest_command: &[],
        image_process_config: &image_process_config,
        mem_gib: Some(4),
        log_level: LogLevel::Off,
        network_mode: NetworkMode::Tsi,
        gpu_mode: GpuMode::Off,
        wayland: false,
        io_uring: false,
        perf: false,
        publish: &[],
        profile: false,
        root: false,
        allocator: AllocatorMode::Mimalloc,
        host_uid: 1000,
        host_gid: 1001,
        vcpus: 2,
        disks: Vec::new(),
        extra_env: Vec::new(),
        host_nix_overlay: Some(HostNixOverlay {
            selected_reference: "localhost/loftd:latest".to_owned(),
            image_digest: "sha256:deadbeef".to_owned(),
            digest_key: "sha256-deadbeef".to_owned(),
            lowerdir: Path::new("/cache/rootfs/nix").to_path_buf(),
            upperdir: Path::new("/state/nix-overlay/upper").to_path_buf(),
            workdir: Path::new("/state/nix-overlay/work").to_path_buf(),
            mergeddir: Path::new("/state/nix-overlay/merged").to_path_buf(),
        }),
        waypipe: None,
        managed_session: None,
    })
    .expect_err("duplicate /nix mount should fail");

    assert!(format!("{err:#}").contains("host /nix overlay owns /nix"));
}

#[test]
fn launch_config_rejects_noncanonical_reserved_target_aliases() {
    let image_process_config = OciProcessConfig::default();
    for target in ["/workspace/", "/workspace/.", "/nix//"] {
        let mut mounts = test_mounts();
        mounts.push(BindMount::directory("/host/alias", "user-alias", target));

        let err = LaunchConfig::build_for_task(LaunchSpec {
            task_rootfs: Path::new("/state/task/rootfs"),
            hostname: "loftd-workspace",
            mounts: &mounts,
            guest_init_override: None,
            guest_init_exec: "/nix/store/hash-loftd/bin/loftd-guest-init",
            guest_command: &[],
            image_process_config: &image_process_config,
            mem_gib: Some(4),
            log_level: LogLevel::Off,
            network_mode: NetworkMode::Tsi,
            gpu_mode: GpuMode::Off,
            wayland: false,
            io_uring: false,
            perf: false,
            publish: &[],
            profile: false,
            root: false,
            allocator: AllocatorMode::Mimalloc,
            host_uid: 1000,
            host_gid: 1001,
            vcpus: 2,
            disks: Vec::new(),
            extra_env: Vec::new(),
            host_nix_overlay: Some(HostNixOverlay {
                selected_reference: "localhost/loftd:latest".to_owned(),
                image_digest: "sha256:deadbeef".to_owned(),
                digest_key: "sha256-deadbeef".to_owned(),
                lowerdir: Path::new("/cache/rootfs/nix").to_path_buf(),
                upperdir: Path::new("/state/nix-overlay/upper").to_path_buf(),
                workdir: Path::new("/state/nix-overlay/work").to_path_buf(),
                mergeddir: Path::new("/state/nix-overlay/merged").to_path_buf(),
            }),
            waypipe: None,
            managed_session: None,
        })
        .expect_err("target aliases should fail before prepared-root normalization");

        assert!(
            format!("{err:#}").contains("canonical absolute path"),
            "unexpected error for {target}: {err:#}"
        );
    }
}

#[test]
fn launch_config_carries_publish_specs_from_launch_spec() {
    let publish = vec!["8080:80".to_owned(), "udp:5353:5353".to_owned()];
    let image_process_config = OciProcessConfig::default();
    let config = LaunchConfig::build_for_task(LaunchSpec {
        task_rootfs: Path::new("/state/task/rootfs"),
        hostname: "loftd-workspace",
        mounts: &test_mounts(),
        guest_init_override: None,
        guest_init_exec: "/nix/store/hash-loftd/bin/loftd-guest-init",
        guest_command: &[],
        image_process_config: &image_process_config,
        mem_gib: Some(4),
        log_level: LogLevel::Off,
        network_mode: NetworkMode::Passt,
        gpu_mode: GpuMode::Off,
        wayland: false,
        io_uring: false,
        perf: false,
        publish: &publish,
        profile: false,
        root: false,
        allocator: AllocatorMode::Mimalloc,
        host_uid: 1000,
        host_gid: 1001,
        vcpus: 2,
        disks: Vec::new(),
        extra_env: Vec::new(),
        host_nix_overlay: None,
        waypipe: None,
        managed_session: None,
    })
    .expect("launch config should build");

    assert_eq!(config.publish, publish);
}

#[test]
fn launch_config_round_trips_publish_entries_in_order() {
    let mut text = String::new();
    push_field(&mut text, "task_rootfs", "/state/task/rootfs");
    push_field(&mut text, "hostname", "loftd-workspace");
    push_field(&mut text, "workspace_source", "/workspace-src");
    push_field(&mut text, "workspace_tag", WORKSPACE_TAG);
    push_field(&mut text, "workspace_target", WORKSPACE_TARGET);
    push_field(&mut text, "ram_mib", "4096");
    push_field(&mut text, "vcpus", "2");
    push_field(&mut text, "log_level", "off");
    push_field(&mut text, "network_mode", "passt");
    push_field(&mut text, "publish.1", "udp:5353:5353");
    push_field(&mut text, "publish.0", "8080:80");
    push_field(&mut text, "workdir", "/workspace");
    push_field(&mut text, "exec_path", "/loftd-guest-init");

    let parsed = LaunchConfig::parse(&text).expect("config should parse");

    assert_eq!(parsed.publish, ["8080:80", "udp:5353:5353"]);
    let serialized = decode_text_for_debug(&parsed.serialize()).expect("debug decode");
    assert!(serialized.contains("publish.0=8080:80\n"));
    assert!(serialized.contains("publish.1=udp:5353:5353\n"));
}

#[test]
fn launch_config_debug_decode_preserves_delimiters_and_escapes_controls() {
    let mut text = String::new();
    push_field(&mut text, "env.0", "KEY=value=with=equals");
    push_field(&mut text, "argv.0", "line\nwith\ttabs");
    text.push_str("future.key=76616c7565\n");

    let decoded = decode_text_for_debug(&text).expect("debug decode should succeed");

    assert_eq!(
        decoded,
        "env.0=KEY=value=with=equals\nargv.0=line\\nwith\\ttabs\nfuture.key=value\n"
    );
}

#[test]
fn launch_config_debug_decode_rejects_malformed_lines() {
    let missing_equals =
        decode_text_for_debug("task_rootfs\n").expect_err("missing separator should fail");
    assert!(format!("{missing_equals:#}").contains("missing '='"));

    let invalid_hex = decode_text_for_debug("task_rootfs=6\n").expect_err("odd hex should fail");
    assert!(format!("{invalid_hex:#}").contains("invalid hex"));
}

#[test]
fn launch_config_legacy_workspace_fields_fall_back_to_single_workspace_mount() {
    let mut text = String::new();
    push_field(&mut text, "task_rootfs", "/state/task/rootfs");
    push_field(&mut text, "hostname", "loftd-workspace");
    push_field(&mut text, "workspace_source", "/workspace-src");
    push_field(&mut text, "workspace_tag", "loftd-workspace");
    push_field(&mut text, "workspace_target", "/workspace");
    push_field(&mut text, "ram_mib", "4096");
    push_field(&mut text, "vcpus", "2");
    push_field(&mut text, "log_level", "off");
    push_field(&mut text, "network_mode", "tsi");
    push_field(&mut text, "workdir", "/workspace");
    push_field(&mut text, "exec_path", "/loftd-guest-init");

    let parsed = LaunchConfig::parse(&text).expect("legacy config should parse");

    assert!(parsed.publish.is_empty());
    assert_eq!(
        parsed.mounts,
        [BindMount::directory(
            "/workspace-src",
            WORKSPACE_TAG,
            WORKSPACE_TARGET
        )]
    );
}

#[test]
fn launch_config_indexed_mounts_default_to_directory_read_write_for_legacy_configs() {
    let mut text = String::new();
    push_field(&mut text, "task_rootfs", "/state/task/rootfs");
    push_field(&mut text, "hostname", "loftd-workspace");
    push_field(&mut text, "mount.0.source", "/workspace-src");
    push_field(&mut text, "mount.0.tag", WORKSPACE_TAG);
    push_field(&mut text, "mount.0.target", WORKSPACE_TARGET);
    push_field(&mut text, "ram_mib", "4096");
    push_field(&mut text, "vcpus", "2");
    push_field(&mut text, "log_level", "off");
    push_field(&mut text, "network_mode", "tsi");
    push_field(&mut text, "workdir", "/workspace");
    push_field(&mut text, "exec_path", "/loftd-guest-init");

    let parsed = LaunchConfig::parse(&text).expect("legacy indexed config should parse");

    assert_eq!(parsed.mounts[0].source_kind, BindMountSourceKind::Directory);
    assert!(!parsed.mounts[0].read_only);
}

#[test]
fn launch_config_rejects_config_codex_mounts() {
    let mut mounts = test_mounts();
    mounts[1].source = Path::new("/home/host/.config/codex").to_path_buf();
    let image_process_config = OciProcessConfig::default();

    let err = LaunchConfig::build_for_task(LaunchSpec {
        task_rootfs: Path::new("/state/task/rootfs"),
        hostname: "loftd-workspace",
        mounts: &mounts,
        guest_init_override: None,
        guest_init_exec: "/nix/store/hash-loftd/bin/loftd-guest-init",
        guest_command: &[],
        image_process_config: &image_process_config,
        mem_gib: Some(4),
        log_level: LogLevel::Off,
        network_mode: NetworkMode::Tsi,
        gpu_mode: GpuMode::Off,
        wayland: false,
        io_uring: false,
        perf: false,
        publish: &[],
        profile: false,
        root: false,
        allocator: AllocatorMode::Mimalloc,
        host_uid: 1000,
        host_gid: 1001,
        vcpus: 2,
        disks: Vec::new(),
        extra_env: Vec::new(),
        host_nix_overlay: None,
        waypipe: None,
        managed_session: None,
    })
    .expect_err("config codex source should be rejected");

    assert!(format!("{err:#}").contains(".config/codex"));
}

#[test]
fn launch_config_requires_guest_init_override_to_be_read_only() {
    let image_process_config = OciProcessConfig::default();

    let err = LaunchConfig::build_for_task(LaunchSpec {
        task_rootfs: Path::new("/state/task/rootfs"),
        hostname: "loftd-workspace",
        mounts: &test_mounts(),
        guest_init_override: Some(GuestInitOverrideMount {
            source: Path::new("/host/override-loftd-guest-init").to_path_buf(),
            target: "/nix/store/hash-loftd/bin/loftd-guest-init".to_owned(),
            read_only: false,
        }),
        guest_init_exec: "/nix/store/hash-loftd/bin/loftd-guest-init",
        guest_command: &[],
        image_process_config: &image_process_config,
        mem_gib: Some(4),
        log_level: LogLevel::Off,
        network_mode: NetworkMode::Tsi,
        gpu_mode: GpuMode::Off,
        wayland: false,
        io_uring: false,
        perf: false,
        publish: &[],
        profile: false,
        root: false,
        allocator: AllocatorMode::Mimalloc,
        host_uid: 1000,
        host_gid: 1001,
        vcpus: 2,
        disks: Vec::new(),
        extra_env: Vec::new(),
        host_nix_overlay: None,
        waypipe: None,
        managed_session: None,
    })
    .expect_err("guest-init override bind must be read-only");

    assert!(format!("{err:#}").contains("read-only"));
}

#[test]
fn launch_config_rejects_guest_init_override_target_that_differs_from_exec_path() {
    let mut text = String::new();
    push_field(&mut text, "task_rootfs", "/state/task/rootfs");
    push_field(&mut text, "hostname", "loftd-workspace");
    push_field(&mut text, "mount.0.source", "/workspace-src");
    push_field(&mut text, "mount.0.tag", WORKSPACE_TAG);
    push_field(&mut text, "mount.0.target", WORKSPACE_TARGET);
    push_field(
        &mut text,
        "guest_init_override_source",
        "/host/override-loftd-guest-init",
    );
    push_field(
        &mut text,
        "guest_init_override_target",
        "/nix/store/wrong/bin/loftd-guest-init",
    );
    push_field(&mut text, "guest_init_override_read_only", "true");
    push_field(&mut text, "ram_mib", "4096");
    push_field(&mut text, "vcpus", "2");
    push_field(&mut text, "log_level", "off");
    push_field(&mut text, "network_mode", "tsi");
    push_field(&mut text, "workdir", "/workspace");
    push_field(
        &mut text,
        "exec_path",
        "/nix/store/hash-loftd/bin/loftd-guest-init",
    );

    let err = LaunchConfig::parse(&text)
        .expect_err("helper config must reject mismatched guest-init override target");

    assert!(format!("{err:#}").contains("must match guest-init exec path"));
}

#[test]
fn launch_config_rejects_unknown_missing_and_malformed_keys() {
    assert!(LaunchConfig::parse("unknown=61\n").is_err());
    assert!(LaunchConfig::parse("task_rootfs=6\n").is_err());
    assert!(LaunchConfig::parse("task_rootfs=2f\n").is_err());
}

#[test]
fn launch_config_rejects_malformed_publish_indexes() {
    let mut text = String::new();
    push_field(&mut text, "task_rootfs", "/state/task/rootfs");
    push_field(&mut text, "hostname", "loftd-workspace");
    push_field(&mut text, "workspace_source", "/workspace-src");
    push_field(&mut text, "workspace_tag", WORKSPACE_TAG);
    push_field(&mut text, "workspace_target", WORKSPACE_TARGET);
    push_field(&mut text, "ram_mib", "4096");
    push_field(&mut text, "vcpus", "2");
    push_field(&mut text, "log_level", "off");
    push_field(&mut text, "network_mode", "tsi");
    push_field(&mut text, "publish.bad", "8080:80");
    push_field(&mut text, "workdir", "/workspace");
    push_field(&mut text, "exec_path", "/loftd-guest-init");

    let err = LaunchConfig::parse(&text).expect_err("bad publish index should fail");

    assert!(format!("{err:#}").contains("publish.bad"));
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
            "LOFTD_MIMALLOC_LIB=/nix/store/libmimalloc.so".to_owned(),
            "LOFTD_GRAPHENE_HARDENED_MALLOC_LIB=/nix/store/libhardened_malloc.so".to_owned(),
            "LOFTD_REAL_PODMAN=/nix/store/podman/bin/podman".to_owned(),
            "NIX_CONFIG=experimental-features = nix-command flakes".to_owned(),
            "SSL_CERT_FILE=/nix/store/cacert/etc/ssl/certs/ca-bundle.crt".to_owned(),
            "NIX_SSL_CERT_FILE=/nix/store/cacert/etc/ssl/certs/ca-bundle.crt".to_owned(),
            "LOFTD_UNRELATED_IMAGE_ENV=ignored".to_owned(),
            "LOFTD_CONTAINERS_STORAGE=image".to_owned(),
        ],
        ..OciProcessConfig::default()
    };

    let config = LaunchConfig::build_for_task(LaunchSpec {
        task_rootfs: Path::new("/state/task/rootfs"),
        hostname: "loftd-workspace",
        mounts: &test_mounts(),
        guest_init_override: None,
        guest_init_exec: "/nix/store/hash-loftd/bin/loftd-guest-init",
        guest_command: &[],
        image_process_config: &image_process_config,
        mem_gib: Some(4),
        log_level: LogLevel::Off,
        network_mode: NetworkMode::Tsi,
        gpu_mode: GpuMode::Off,
        wayland: false,
        io_uring: false,
        perf: false,
        publish: &[],
        profile: false,
        root: false,
        allocator: AllocatorMode::Mimalloc,
        host_uid: 1000,
        host_gid: 1001,
        vcpus: 2,
        disks: Vec::new(),
        extra_env: vec![("LOFTD_CONTAINERS_STORAGE".to_owned(), "disk".to_owned())],
        host_nix_overlay: None,
        waypipe: None,
        managed_session: None,
    })
    .expect("launch config should build");

    assert_eq!(
        config.env,
        [("KRUN_CONFIG".to_owned(), "/.loftd_config.json".to_owned())]
    );
    assert!(config.guest_config_env_contains("PATH", "/nix/store/fish/bin"));
    assert!(config.guest_config_env_contains("LOFTD_HOST_UID", "1000"));
    assert!(config.guest_config_env_contains("LOFTD_CONTAINERS_STORAGE", "disk"));
    assert!(config.guest_config_env_contains("LOFTD_FISH_CONFIG_SOURCE", "/nix/store/fish-config"));
    assert!(
        config
            .guest_config_env_contains("LOFTD_STARSHIP_CONFIG_SOURCE", "/nix/store/starship.toml")
    );
    assert!(config.guest_config_env_contains("LOFTD_MIMALLOC_LIB", "/nix/store/libmimalloc.so"));
    assert!(config.guest_config_env_contains(
        "LOFTD_GRAPHENE_HARDENED_MALLOC_LIB",
        "/nix/store/libhardened_malloc.so"
    ));
    assert!(config.guest_config_env_contains("LOFTD_REAL_PODMAN", "/nix/store/podman/bin/podman"));
    assert!(
        config
            .guest_config_env_contains("NIX_CONFIG", "experimental-features = nix-command flakes")
    );
    assert!(config.guest_config_env_contains(
        "SSL_CERT_FILE",
        "/nix/store/cacert/etc/ssl/certs/ca-bundle.crt"
    ));
    assert!(config.guest_config_env_contains(
        "NIX_SSL_CERT_FILE",
        "/nix/store/cacert/etc/ssl/certs/ca-bundle.crt"
    ));
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
fn launch_config_emits_allocator_selector() {
    let image_process_config = OciProcessConfig {
        env: vec![
            "LOFTD_MIMALLOC_LIB=/nix/store/libmimalloc.so".to_owned(),
            "LOFTD_GRAPHENE_HARDENED_MALLOC_LIB=/nix/store/libhardened_malloc.so".to_owned(),
        ],
        ..OciProcessConfig::default()
    };

    for (allocator, expected) in [
        (AllocatorMode::Mimalloc, "mimalloc"),
        (AllocatorMode::Hardened, "hardened"),
        (AllocatorMode::Glibc, "glibc"),
    ] {
        let config = LaunchConfig::build_for_task(LaunchSpec {
            task_rootfs: Path::new("/state/task/rootfs"),
            hostname: "loftd-workspace",
            mounts: &test_mounts(),
            guest_init_override: None,
            guest_init_exec: "/nix/store/hash-loftd/bin/loftd-guest-init",
            guest_command: &[],
            image_process_config: &image_process_config,
            mem_gib: Some(4),
            log_level: LogLevel::Off,
            network_mode: NetworkMode::Tsi,
            gpu_mode: GpuMode::Off,
            wayland: false,
            io_uring: false,
            perf: false,
            publish: &[],
            profile: false,
            root: false,
            allocator,
            host_uid: 1000,
            host_gid: 1001,
            vcpus: 2,
            disks: Vec::new(),
            extra_env: Vec::new(),
            host_nix_overlay: None,
            waypipe: None,
            managed_session: None,
        })
        .expect("launch config should build");

        assert!(config.guest_config_env_contains("LOFTD_NIX_ALLOCATOR", expected));
        assert!(
            config.guest_config_env_contains("LOFTD_MIMALLOC_LIB", "/nix/store/libmimalloc.so")
        );
        assert!(config.guest_config_env_contains(
            "LOFTD_GRAPHENE_HARDENED_MALLOC_LIB",
            "/nix/store/libhardened_malloc.so"
        ));
        assert!(
            config
                .env
                .iter()
                .all(|(key, value)| key != "LOFTD_NIX_ALLOCATOR" && !value.contains("LD_PRELOAD"))
        );
    }
}

#[test]
fn guest_debug_env_follows_effective_log_level() {
    let image_process_config = OciProcessConfig::default();
    let config = LaunchConfig::build_for_task(LaunchSpec {
        task_rootfs: Path::new("/state/task/rootfs"),
        hostname: "loftd-workspace",
        mounts: &test_mounts(),
        guest_init_override: None,
        guest_init_exec: "/nix/store/hash-loftd/bin/loftd-guest-init",
        guest_command: &[],
        image_process_config: &image_process_config,
        mem_gib: Some(4),
        log_level: LogLevel::Info,
        network_mode: NetworkMode::Tsi,
        gpu_mode: GpuMode::Off,
        wayland: false,
        io_uring: false,
        perf: false,
        publish: &[],
        profile: false,
        root: false,
        allocator: AllocatorMode::Mimalloc,
        host_uid: 1000,
        host_gid: 1001,
        vcpus: 2,
        disks: Vec::new(),
        extra_env: Vec::new(),
        host_nix_overlay: None,
        waypipe: None,
        managed_session: None,
    })
    .expect("launch config should build");
    assert!(!config.guest_config_env_contains("LOFTD_GUEST_DEBUG", "1"));

    let config = LaunchConfig::build_for_task(LaunchSpec {
        task_rootfs: Path::new("/state/task/rootfs"),
        hostname: "loftd-workspace",
        mounts: &test_mounts(),
        guest_init_override: None,
        guest_init_exec: "/nix/store/hash-loftd/bin/loftd-guest-init",
        guest_command: &[],
        image_process_config: &image_process_config,
        mem_gib: Some(4),
        log_level: LogLevel::Trace,
        network_mode: NetworkMode::Tsi,
        gpu_mode: GpuMode::Off,
        wayland: false,
        io_uring: false,
        perf: false,
        publish: &[],
        profile: false,
        root: false,
        allocator: AllocatorMode::Mimalloc,
        host_uid: 1000,
        host_gid: 1001,
        vcpus: 2,
        disks: Vec::new(),
        extra_env: Vec::new(),
        host_nix_overlay: None,
        waypipe: None,
        managed_session: None,
    })
    .expect("launch config should build");
    assert!(config.guest_config_env_contains("LOFTD_GUEST_DEBUG", "1"));
}

#[test]
fn profile_env_does_not_raise_guest_debug_level() {
    let image_process_config = OciProcessConfig::default();
    let config = LaunchConfig::build_for_task(LaunchSpec {
        task_rootfs: Path::new("/state/task/rootfs"),
        hostname: "loftd-workspace",
        mounts: &test_mounts(),
        guest_init_override: None,
        guest_init_exec: "/nix/store/hash-loftd/bin/loftd-guest-init",
        guest_command: &[],
        image_process_config: &image_process_config,
        mem_gib: Some(4),
        log_level: LogLevel::Info,
        network_mode: NetworkMode::Tsi,
        gpu_mode: GpuMode::Off,
        wayland: false,
        io_uring: false,
        perf: false,
        publish: &[],
        profile: true,
        root: false,
        allocator: AllocatorMode::Mimalloc,
        host_uid: 1000,
        host_gid: 1001,
        vcpus: 2,
        disks: Vec::new(),
        extra_env: Vec::new(),
        host_nix_overlay: None,
        waypipe: None,
        managed_session: None,
    })
    .expect("launch config should build");

    assert!(config.guest_config_env_contains("LOFTD_GUEST_PROFILE", "1"));
    assert!(!config.guest_config_env_contains("LOFTD_GUEST_DEBUG", "1"));
}

#[test]
fn passt_mode_sets_guest_passt_dns_gate() {
    let image_process_config = OciProcessConfig::default();
    let config = LaunchConfig::build_for_task(LaunchSpec {
        task_rootfs: Path::new("/state/task/rootfs"),
        hostname: "loftd-workspace",
        mounts: &test_mounts(),
        guest_init_override: None,
        guest_init_exec: "/nix/store/hash-loftd/bin/loftd-guest-init",
        guest_command: &[],
        image_process_config: &image_process_config,
        mem_gib: Some(4),
        log_level: LogLevel::Off,
        network_mode: NetworkMode::Passt,
        gpu_mode: GpuMode::Off,
        wayland: false,
        io_uring: false,
        perf: false,
        publish: &[],
        profile: false,
        root: false,
        allocator: AllocatorMode::Mimalloc,
        host_uid: 1000,
        host_gid: 1001,
        vcpus: 2,
        disks: Vec::new(),
        extra_env: Vec::new(),
        host_nix_overlay: None,
        waypipe: None,
        managed_session: None,
    })
    .expect("launch config should build");

    assert_eq!(config.network_mode, NetworkMode::Passt);
    assert!(config.guest_config_env_contains("LOFTD_USE_PASST", "1"));
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
        hostname: "loftd-workspace",
        mounts: &test_mounts(),
        guest_init_override: None,
        guest_init_exec: "/nix/store/hash-loftd/bin/loftd-guest-init",
        guest_command: &[],
        image_process_config: &image_process_config,
        mem_gib: Some(4),
        log_level: LogLevel::Off,
        network_mode: NetworkMode::Tsi,
        gpu_mode: GpuMode::Off,
        wayland: false,
        io_uring: false,
        perf: false,
        publish: &[],
        profile: false,
        root: false,
        allocator: AllocatorMode::Mimalloc,
        host_uid: 1000,
        host_gid: 1001,
        vcpus: 2,
        disks: Vec::new(),
        extra_env: vec![(
            "LOFTD_JSON_TEST".to_owned(),
            "line\nslash\\tab\t".to_owned(),
        )],
        host_nix_overlay: None,
        waypipe: None,
        managed_session: None,
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
            hostname: "loftd-workspace",
            mounts: &test_mounts(),
            guest_init_override: None,
            guest_init_exec: "/nix/store/hash-loftd/bin/loftd-guest-init",
            guest_command: &[],
            image_process_config,
            mem_gib: Some(4),
            log_level: LogLevel::Off,
            network_mode: NetworkMode::Tsi,
            gpu_mode: GpuMode::Off,
            wayland: false,
            io_uring: false,
            perf: false,
            publish: &[],
            profile: false,
            root: false,
            allocator: AllocatorMode::Mimalloc,
            host_uid: 1000,
            host_gid: 1001,
            vcpus: 2,
            disks: Vec::new(),
            extra_env: Vec::new(),
            host_nix_overlay: None,
            waypipe: None,
            managed_session: None,
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
        hostname: "loftd-workspace",
        mounts: &test_mounts(),
        guest_init_override: None,
        guest_init_exec: "/nix/store/hash-loftd/bin/loftd-guest-init",
        guest_command: &[],
        image_process_config: &image_process_config,
        mem_gib: Some(4),
        log_level: LogLevel::Off,
        network_mode: NetworkMode::Tsi,
        gpu_mode: GpuMode::Off,
        wayland: false,
        io_uring: false,
        perf: false,
        publish: &[],
        profile: false,
        root: false,
        allocator: AllocatorMode::Mimalloc,
        host_uid: 1000,
        host_gid: 1001,
        vcpus: 2,
        disks: Vec::new(),
        extra_env: Vec::new(),
        host_nix_overlay: None,
        waypipe: None,
        managed_session: None,
    })
    .expect("launch config should build");

    assert_eq!(config.argv, ["enter", "bash", "-lc", "echo image"]);
}
