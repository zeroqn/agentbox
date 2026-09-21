use super::*;
use std::cell::RefCell;

const MEMINFO_8_GIB: &str = "MemTotal:        8388608 kB\nMemFree:         1048576 kB\n";
const FOUR_GIB: u64 = 4 * 1024 * 1024 * 1024;

#[test]
fn plan_swap_size_halves_guest_ram() {
    assert_eq!(plan_swap_size(8 * 1024 * 1024 * 1024), FOUR_GIB);
}

#[test]
fn plan_swap_size_floors_tiny_guests() {
    assert_eq!(plan_swap_size(64 * 1024 * 1024), MIN_SWAP_BYTES);
}

#[test]
fn parse_mem_total_reads_kibibytes_from_meminfo() {
    assert_eq!(
        parse_mem_total(MEMINFO_8_GIB).unwrap(),
        8 * 1024 * 1024 * 1024
    );
}

#[test]
fn parse_mem_total_rejects_meminfo_without_the_line() {
    let err = parse_mem_total("MemFree: 1024 kB\n").expect_err("MemTotal is required");

    assert!(format!("{err:#}").contains("MemTotal"));
}

#[test]
fn swap_active_matches_the_device_column_only() {
    let swaps = "Filename\t\t\t\tType\t\tSize\t\tUsed\t\tPriority\n\
                 /dev/zram0                              partition\t4194300\t0\t100\n";

    assert!(swap_active(swaps, DEVICE));
    assert!(!swap_active(swaps, "/dev/zram1"));
    assert!(!swap_active(
        "Filename\t\t\t\tType\t\tSize\t\tUsed\t\tPriority\n",
        DEVICE
    ));
}

#[test]
fn activation_sizes_signs_and_activates_the_zram_device() {
    let backend = FakeSwapBackend::new();

    let report = activate_with(&backend).expect("zram activation should succeed");

    assert_eq!(report.state, GuestSwapState::Ready);
    assert_eq!(report.size_bytes, Some(FOUR_GIB));
    assert_eq!(
        backend.operations(),
        [
            "exists:/sys/block/zram0/disksize",
            "read:/proc/swaps",
            "read:/proc/meminfo",
            "write:/sys/block/zram0/disksize=4294967296",
            "run:mkswap /dev/zram0",
            "run:swapon -p 100 /dev/zram0",
        ]
    );
}

#[test]
fn activation_reports_the_active_device_without_resizing_it() {
    let backend = FakeSwapBackend::with_active_device("4294967296\n");

    let report = activate_with(&backend).expect("an active device is not an error");

    assert_eq!(report.state, GuestSwapState::Ready);
    assert_eq!(report.size_bytes, Some(FOUR_GIB));
    assert_eq!(
        backend.operations(),
        [
            "exists:/sys/block/zram0/disksize",
            "read:/proc/swaps",
            "read:/sys/block/zram0/disksize",
        ]
    );
}

#[test]
fn ensure_records_a_ready_device_in_the_status_file() {
    let backend = FakeSwapBackend::new();

    ensure_with(&backend);

    assert_eq!(
        backend.status(),
        format!("state=ready\ndevice={DEVICE}\nsize_bytes={FOUR_GIB}\npriority={SWAP_PRIORITY}\n")
    );
}

#[test]
fn ensure_treats_a_kernel_without_zram_as_unavailable_and_boots_anyway() {
    let backend = FakeSwapBackend::without_zram();

    ensure_with(&backend);

    assert_eq!(
        backend.status(),
        format!("state=unavailable\ndevice={DEVICE}\npriority={SWAP_PRIORITY}\n")
    );
    assert_eq!(
        backend.operations(),
        [
            "exists:/sys/block/zram0/disksize",
            "status:state=unavailable",
        ]
    );
}

#[test]
fn ensure_records_a_failed_activation_without_failing_the_session() {
    let backend = FakeSwapBackend::failing_on("mkswap");

    ensure_with(&backend);

    let status = backend.status();
    assert!(status.contains("state=failed\n"), "status was: {status}");
    assert!(status.contains("error="), "status was: {status}");
    assert!(status.contains("mkswap"), "status was: {status}");
}

struct FakeSwapBackend {
    operations: RefCell<Vec<String>>,
    status: RefCell<Option<String>>,
    zram_present: bool,
    swaps: RefCell<String>,
    meminfo: RefCell<String>,
    device_size: RefCell<Option<String>>,
    failing: RefCell<Option<String>>,
}

impl FakeSwapBackend {
    fn new() -> Self {
        Self {
            operations: RefCell::new(Vec::new()),
            status: RefCell::new(None),
            zram_present: true,
            swaps: RefCell::new("Filename\t\t\t\tType\t\tSize\t\tUsed\t\tPriority\n".to_owned()),
            meminfo: RefCell::new(MEMINFO_8_GIB.to_owned()),
            device_size: RefCell::new(None),
            failing: RefCell::new(None),
        }
    }

    fn without_zram() -> Self {
        Self {
            zram_present: false,
            ..Self::new()
        }
    }

    fn with_active_device(size: &str) -> Self {
        let backend = Self::new();
        *backend.swaps.borrow_mut() = format!(
            "Filename\t\t\t\tType\t\tSize\t\tUsed\t\tPriority\n\
             {DEVICE}                              partition\t4194300\t0\t100\n"
        );
        *backend.device_size.borrow_mut() = Some(size.to_owned());
        backend
    }

    fn failing_on(program: &str) -> Self {
        let backend = Self::new();
        *backend.failing.borrow_mut() = Some(program.to_owned());
        backend
    }

    fn record(&self, operation: impl Into<String>) {
        self.operations.borrow_mut().push(operation.into());
    }

    fn operations(&self) -> Vec<String> {
        self.operations.borrow().clone()
    }

    fn status(&self) -> String {
        self.status.borrow().clone().unwrap_or_default()
    }
}

impl SwapBackend for FakeSwapBackend {
    fn path_exists(&self, path: &Path) -> bool {
        self.record(format!("exists:{}", path.display()));
        self.zram_present
    }

    fn read(&self, path: &Path) -> Result<String> {
        self.record(format!("read:{}", path.display()));
        match path.to_str() {
            Some(SWAPS_PATH) => Ok(self.swaps.borrow().clone()),
            Some(MEMINFO_PATH) => Ok(self.meminfo.borrow().clone()),
            Some(DEVICE_SIZE_PATH) => self
                .device_size
                .borrow()
                .clone()
                .ok_or_else(|| anyhow!("zram device has no size")),
            other => Err(anyhow!("unexpected read of {other:?}")),
        }
    }

    fn write(&self, path: &Path, value: &str) -> Result<()> {
        self.record(format!("write:{}={}", path.display(), value.trim()));
        if self.failing.borrow().as_deref() == Some("write") {
            return Err(anyhow!("failed to write {}", path.display()));
        }
        *self.device_size.borrow_mut() = Some(value.trim().to_owned());
        Ok(())
    }

    fn run(&self, program: &str, args: &[&str]) -> Result<()> {
        self.record(format!("run:{} {}", program, args.join(" ")));
        if self.failing.borrow().as_deref() == Some(program) {
            return Err(anyhow!("{program} exited with status 1"));
        }
        Ok(())
    }

    fn record_status(&self, contents: &str) {
        self.record(format!(
            "status:{}",
            contents.lines().next().unwrap_or_default()
        ));
        *self.status.borrow_mut() = Some(contents.to_owned());
    }
}
