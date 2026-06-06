use super::*;
use crate::logging::LogLevel;
use crate::runtime::launch::config::{
    BindMount, CARGO_TAG, CARGO_TARGET, CODEX_TAG, CODEX_TARGET, DiskAttachment, LaunchConfig,
    LaunchSpec, NetworkMode, PI_TAG, PI_TARGET, SCCACHE_TAG, SCCACHE_TARGET, WORKSPACE_TAG,
    WORKSPACE_TARGET,
};
use anyhow::Result;
use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Call {
    CreateCtx,
    FreeCtx(u32),
    InitLog(u32),
    SetVmConfig(u32, u8, u32),
    SetRoot(u32, String),
    AddDisk(u32, String, String, bool),
    AddNetUnixstream(u32, String),
    DisableImplicitConsole(u32),
    AddVirtioConsoleDefault(u32, i32, i32, i32),
    SetWorkdir(u32, String),
    SetExec(u32, String, Vec<String>, Vec<(String, String)>),
    SetProfilePath(u32, String),
    StartEnter(u32),
}

#[derive(Clone)]
struct FakeLibkrunApi {
    calls: Rc<RefCell<Vec<Call>>>,
    fail_call: Option<&'static str>,
}

impl FakeLibkrunApi {
    fn new(calls: Rc<RefCell<Vec<Call>>>) -> Self {
        Self {
            calls,
            fail_call: None,
        }
    }

    fn failing(calls: Rc<RefCell<Vec<Call>>>, fail_call: &'static str) -> Self {
        Self {
            calls,
            fail_call: Some(fail_call),
        }
    }

    fn rc(&self, call: &'static str) -> i32 {
        if self.fail_call == Some(call) { -22 } else { 0 }
    }
}

impl LibkrunApi for FakeLibkrunApi {
    fn create_ctx(&mut self) -> Result<u32> {
        self.calls.borrow_mut().push(Call::CreateCtx);
        Ok(7)
    }

    fn free_ctx(&mut self, ctx_id: u32) -> Result<()> {
        self.calls.borrow_mut().push(Call::FreeCtx(ctx_id));
        Ok(())
    }

    fn init_log(&mut self, level: u32) -> Result<i32> {
        self.calls.borrow_mut().push(Call::InitLog(level));
        Ok(self.rc("libkrun_log_init"))
    }

    fn set_vm_config(&mut self, ctx_id: u32, vcpus: u8, ram_mib: u32) -> Result<i32> {
        self.calls
            .borrow_mut()
            .push(Call::SetVmConfig(ctx_id, vcpus, ram_mib));
        Ok(self.rc("krun_set_vm_config"))
    }

    fn set_root(&mut self, ctx_id: u32, root_path: &Path) -> Result<i32> {
        self.calls
            .borrow_mut()
            .push(Call::SetRoot(ctx_id, root_path.display().to_string()));
        Ok(self.rc("krun_set_root"))
    }

    fn add_disk(
        &mut self,
        ctx_id: u32,
        block_id: &str,
        disk_path: &Path,
        read_only: bool,
    ) -> Result<i32> {
        self.calls.borrow_mut().push(Call::AddDisk(
            ctx_id,
            block_id.to_owned(),
            disk_path.display().to_string(),
            read_only,
        ));
        Ok(self.rc("krun_add_disk"))
    }

    fn disable_implicit_console(&mut self, ctx_id: u32) -> Result<i32> {
        self.calls
            .borrow_mut()
            .push(Call::DisableImplicitConsole(ctx_id));
        Ok(self.rc("krun_disable_implicit_console"))
    }

    fn add_virtio_console_default(
        &mut self,
        ctx_id: u32,
        input_fd: i32,
        output_fd: i32,
        err_fd: i32,
    ) -> Result<i32> {
        self.calls.borrow_mut().push(Call::AddVirtioConsoleDefault(
            ctx_id, input_fd, output_fd, err_fd,
        ));
        Ok(self.rc("krun_add_virtio_console_default"))
    }

    fn add_net_unixstream(&mut self, ctx_id: u32, socket_path: &Path) -> Result<i32> {
        self.calls.borrow_mut().push(Call::AddNetUnixstream(
            ctx_id,
            socket_path.display().to_string(),
        ));
        Ok(self.rc("krun_add_net_unixstream"))
    }

    fn set_workdir(&mut self, ctx_id: u32, workdir: &str) -> Result<i32> {
        self.calls
            .borrow_mut()
            .push(Call::SetWorkdir(ctx_id, workdir.to_owned()));
        Ok(self.rc("krun_set_workdir"))
    }

    fn set_exec(
        &mut self,
        ctx_id: u32,
        exec_path: &str,
        argv: &[String],
        env: &[(String, String)],
    ) -> Result<i32> {
        self.calls.borrow_mut().push(Call::SetExec(
            ctx_id,
            exec_path.to_owned(),
            argv.to_vec(),
            env.to_vec(),
        ));
        Ok(self.rc("krun_set_exec"))
    }

    fn set_profile_path(&mut self, ctx_id: u32, profile_path: &Path) -> Result<i32> {
        self.calls.borrow_mut().push(Call::SetProfilePath(
            ctx_id,
            profile_path.display().to_string(),
        ));
        Ok(self.rc("krun_set_profile_path"))
    }

    fn start_enter(&mut self, ctx_id: u32) -> Result<i32> {
        self.calls.borrow_mut().push(Call::StartEnter(ctx_id));
        Ok(self.rc("krun_start_enter"))
    }
}

fn config() -> LaunchConfig {
    LaunchConfig::build_for_task(LaunchSpec {
        task_rootfs: Path::new("/rootfs"),
        hostname: "loftd-workspace",
        mounts: &test_mounts(),
        guest_init_override: None,
        guest_init_exec: "/nix/store/hash-loftd/bin/loftd-guest-init",
        guest_command: &[],
        image_process_config:
            &crate::runtime::session::rootfs::image_source::OciProcessConfig::default(),
        mem_gib: Some(4),
        log_level: LogLevel::Debug,
        network_mode: NetworkMode::Tsi,
        profile: false,
        root: false,
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
        extra_env: Vec::new(),
    })
    .expect("config should build")
}

fn passt_config() -> LaunchConfig {
    LaunchConfig {
        network_mode: NetworkMode::Passt,
        ..config().with_passt_socket(Path::new("/tmp/task/passt.sock").to_path_buf())
    }
}

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
fn libkrun_loader_prefers_explicit_library_override_before_sonames() {
    assert_eq!(
        planned_libkrun_load_order(Some("/tmp/libkrun-custom.so")),
        vec!["/tmp/libkrun-custom.so"]
    );
    assert_eq!(
        planned_libkrun_load_order(None),
        vec!["libkrun.so.1", "libkrun.so"]
    );
    assert_eq!(
        planned_libkrun_load_order(Some("")),
        vec!["libkrun.so.1", "libkrun.so"]
    );
}

#[test]
fn compat_net_features_match_libkrun_header_contract() {
    assert_eq!(
        LOFTD_LIBKRUN_COMPAT_NET_FEATURES,
        (1 << 0) | (1 << 1) | (1 << 7) | (1 << 10) | (1 << 11) | (1 << 14)
    );
}

#[test]
fn fake_api_records_direct_libkrun_v1_call_order() {
    let calls = Rc::new(RefCell::new(Vec::new()));
    DirectLibkrunLauncher::new(FakeLibkrunApi::new(calls.clone()))
        .start_enter(&config())
        .expect("launch should succeed");

    let calls = calls.borrow();
    assert_eq!(calls[0], Call::InitLog(4));
    assert_eq!(calls[1], Call::CreateCtx);
    assert_eq!(calls[2], Call::SetVmConfig(7, 2, 4096));
    assert_eq!(calls[3], Call::SetRoot(7, "/rootfs".to_owned()));
    assert_eq!(
        calls[4],
        Call::AddDisk(
            7,
            "loftd-nix".to_owned(),
            "/state/loftd-nix.raw".to_owned(),
            false,
        )
    );
    assert_eq!(
        calls[5],
        Call::AddDisk(
            7,
            "loftd-containers".to_owned(),
            "/state/loftd-containers.raw".to_owned(),
            false,
        )
    );
    assert_eq!(calls[6], Call::DisableImplicitConsole(7));
    assert_eq!(calls[7], Call::AddVirtioConsoleDefault(7, 0, 1, 2));
    assert_eq!(calls[8], Call::SetWorkdir(7, "/workspace".to_owned()));
    assert_eq!(
        calls[9],
        Call::SetExec(
            7,
            "/nix/store/hash-loftd/bin/loftd-guest-init".to_owned(),
            vec!["enter".to_owned(), "fish".to_owned(), "-l".to_owned()],
            vec![("KRUN_CONFIG".to_owned(), "/.loftd_config.json".to_owned())]
        )
    );
    assert_eq!(calls[10], Call::StartEnter(7));
    assert_eq!(
        calls.len(),
        11,
        "TSI default must not call krun_set_env, passt APIs, or per-bind virtiofs devices"
    );
}

#[test]
fn passt_mode_adds_unixstream_before_start() {
    let calls = Rc::new(RefCell::new(Vec::new()));
    DirectLibkrunLauncher::new(FakeLibkrunApi::new(calls.clone()))
        .start_enter(&passt_config())
        .expect("launch should succeed");

    let calls = calls.borrow();
    let net_index = calls
        .iter()
        .position(|call| matches!(call, Call::AddNetUnixstream(..)))
        .expect("passt mode should add net unixstream");
    let start_index = calls
        .iter()
        .position(|call| matches!(call, Call::StartEnter(..)))
        .expect("launch should start");

    assert_eq!(
        calls[net_index],
        Call::AddNetUnixstream(7, "/tmp/task/passt.sock".to_owned())
    );
    assert!(net_index < start_index);
}

#[test]
fn pre_enter_hook_runs_after_setup_and_before_start() {
    let calls = Rc::new(RefCell::new(Vec::new()));
    DirectLibkrunLauncher::new(FakeLibkrunApi::new(calls.clone()))
        .start_enter_with_pre_enter_hook(&config(), || {
            let calls = calls.borrow();
            assert!(calls.iter().any(|call| matches!(call, Call::SetExec(..))));
            assert!(
                !calls
                    .iter()
                    .any(|call| matches!(call, Call::StartEnter(..)))
            );
        })
        .expect("launch should succeed");

    let calls = calls.borrow();
    let set_exec_index = calls
        .iter()
        .position(|call| matches!(call, Call::SetExec(..)))
        .expect("setup should configure exec before hook");
    let start_index = calls
        .iter()
        .position(|call| matches!(call, Call::StartEnter(..)))
        .expect("launch should start after hook");
    assert!(set_exec_index < start_index);
}

#[test]
fn profile_path_is_set_after_exec_and_before_pre_enter_hook() {
    let calls = Rc::new(RefCell::new(Vec::new()));
    DirectLibkrunLauncher::new(FakeLibkrunApi::new(calls.clone()))
        .start_enter_profiled_with_pre_enter_hook(
            &config(),
            Some(Path::new("/tmp/vm-worker-host-profile.tsv")),
            || {
                let calls = calls.borrow();
                assert!(calls.iter().any(|call| matches!(call, Call::SetExec(..))));
                assert!(
                    calls
                        .iter()
                        .any(|call| matches!(call, Call::SetProfilePath(..)))
                );
                assert!(
                    !calls
                        .iter()
                        .any(|call| matches!(call, Call::StartEnter(..)))
                );
            },
        )
        .expect("launch should succeed");

    let calls = calls.borrow();
    let set_exec_index = calls
        .iter()
        .position(|call| matches!(call, Call::SetExec(..)))
        .expect("setup should configure exec");
    let set_profile_index = calls
        .iter()
        .position(|call| matches!(call, Call::SetProfilePath(..)))
        .expect("profile path should be configured");
    let start_index = calls
        .iter()
        .position(|call| matches!(call, Call::StartEnter(..)))
        .expect("launch should start");

    assert_eq!(
        calls[set_profile_index],
        Call::SetProfilePath(7, "/tmp/vm-worker-host-profile.tsv".to_owned())
    );
    assert!(set_exec_index < set_profile_index);
    assert!(set_profile_index < start_index);
}

#[test]
fn passt_mode_requires_prepared_socket_path() {
    let mut config = config();
    config.network_mode = NetworkMode::Passt;
    let calls = Rc::new(RefCell::new(Vec::new()));
    let err = DirectLibkrunLauncher::new(FakeLibkrunApi::new(calls.clone()))
        .start_enter(&config)
        .expect_err("missing socket should fail setup");

    assert!(format!("{err:#}").contains("prepared passt unix socket"));
    assert!(calls.borrow().contains(&Call::FreeCtx(7)));
}

#[test]
fn setup_failure_is_classified_and_frees_context_before_start() {
    let calls = Rc::new(RefCell::new(Vec::new()));
    let err = DirectLibkrunLauncher::new(FakeLibkrunApi::failing(calls.clone(), "krun_set_root"))
        .start_enter(&config())
        .expect_err("setup failure should fail");

    assert!(format!("{err:#}").contains("libkrun setup failed"));
    let calls = calls.borrow();
    assert!(calls.contains(&Call::FreeCtx(7)));
    assert!(!calls.contains(&Call::StartEnter(7)));
}

#[test]
fn console_registration_failure_frees_context_before_exec() {
    let calls = Rc::new(RefCell::new(Vec::new()));
    let err = DirectLibkrunLauncher::new(FakeLibkrunApi::failing(
        calls.clone(),
        "krun_add_virtio_console_default",
    ))
    .start_enter(&config())
    .expect_err("console setup failure should fail");

    assert!(format!("{err:#}").contains("libkrun setup failed"));
    let calls = calls.borrow();
    assert!(calls.contains(&Call::FreeCtx(7)));
    assert!(!calls.iter().any(|call| matches!(call, Call::SetExec(..))));
    assert!(!calls.contains(&Call::StartEnter(7)));
}

#[test]
fn start_failure_is_classified() {
    let calls = Rc::new(RefCell::new(Vec::new()));
    let err =
        DirectLibkrunLauncher::new(FakeLibkrunApi::failing(calls.clone(), "krun_start_enter"))
            .start_enter(&config())
            .expect_err("start failure should fail");

    assert!(format!("{err:#}").contains("libkrun start failed"));
    assert!(calls.borrow().contains(&Call::StartEnter(7)));
}
