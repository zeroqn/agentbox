use anyhow::{Context, Result, anyhow, bail};
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const ACTIVE_RECORD_FILE: &str = "active-task";
const BOOT_ID_PATH: &str = "/proc/sys/kernel/random/boot_id";
const PROC_ROOT: &str = "/proc";
const TERM_GRACE: Duration = Duration::from_millis(500);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TaskControlCommand {
    Ps,
    Kill { task_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ActiveTaskSpec {
    pub(crate) task_id: String,
    pub(crate) workspace_slug: String,
    pub(crate) workspace_dir: PathBuf,
    pub(crate) task_dir: PathBuf,
    pub(crate) image_reference: String,
    pub(crate) image_digest: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProcessIdentity {
    pub(crate) pid: u32,
    pub(crate) pgid: u32,
    pub(crate) sid: u32,
    pub(crate) proc_start_time_ticks: u64,
    pub(crate) boot_id: String,
}

impl ProcessIdentity {
    pub(crate) fn from_spawned_process(pid: u32, pgid: u32, sid: u32) -> Result<Self> {
        read_process_identity(
            Path::new(PROC_ROOT),
            Path::new(BOOT_ID_PATH),
            pid,
            pgid,
            sid,
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ActiveTaskRecord {
    pub(crate) task_id: String,
    pub(crate) workspace_slug: String,
    pub(crate) workspace_dir: PathBuf,
    pub(crate) task_dir: PathBuf,
    pub(crate) image_reference: String,
    pub(crate) image_digest: Option<String>,
    pub(crate) started_at_unix_secs: u64,
    pub(crate) process: ProcessIdentity,
}

impl ActiveTaskRecord {
    fn from_spec(spec: ActiveTaskSpec, process: ProcessIdentity, now: SystemTime) -> Result<Self> {
        let started_at_unix_secs = now
            .duration_since(UNIX_EPOCH)
            .context("system clock is before unix epoch")?
            .as_secs();
        Ok(Self {
            task_id: spec.task_id,
            workspace_slug: spec.workspace_slug,
            workspace_dir: spec.workspace_dir,
            task_dir: spec.task_dir,
            image_reference: spec.image_reference,
            image_digest: spec.image_digest,
            started_at_unix_secs,
            process,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActiveTaskStatus {
    Running,
    Stale,
    PidReused,
    Unreadable,
}

impl ActiveTaskStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Stale => "stale",
            Self::PidReused => "pid-reused",
            Self::Unreadable => "unreadable",
        }
    }
}

pub(crate) trait ProcessInspector {
    fn status(&self, identity: &ProcessIdentity) -> ActiveTaskStatus;
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ProcfsInspector;

impl ProcessInspector for ProcfsInspector {
    fn status(&self, identity: &ProcessIdentity) -> ActiveTaskStatus {
        inspect_process_identity(Path::new(PROC_ROOT), Path::new(BOOT_ID_PATH), identity)
            .unwrap_or(ActiveTaskStatus::Unreadable)
    }
}

pub(crate) trait TaskSignaler {
    fn signal_process_group(&mut self, pgid: u32, signal: i32) -> Result<()>;
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct HostTaskSignaler;

impl TaskSignaler for HostTaskSignaler {
    fn signal_process_group(&mut self, pgid: u32, signal: i32) -> Result<()> {
        let pgid = i32::try_from(pgid).context("process group id does not fit in i32")?;
        if pgid <= 1 {
            bail!("refusing to signal unsafe process group id {pgid}");
        }
        let target = -pgid;
        let rc = unsafe { libc::kill(target, signal) };
        if rc == 0 {
            return Ok(());
        }
        Err(std::io::Error::last_os_error())
            .with_context(|| format!("failed to send signal {signal} to process group {pgid}"))
    }
}

pub(crate) fn write_active_task(spec: ActiveTaskSpec, process: ProcessIdentity) -> Result<()> {
    let record = ActiveTaskRecord::from_spec(spec, process, SystemTime::now())?;
    write_active_task_record(&record)
}

pub(crate) fn remove_active_task(task_dir: &Path) -> Result<()> {
    let path = active_record_path(task_dir);
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err).with_context(|| format!("failed to remove '{}'", path.display())),
    }
}

pub(crate) fn run_task_control_command(
    command: TaskControlCommand,
    app_dir: &Path,
) -> Result<String> {
    match command {
        TaskControlCommand::Ps => render_ps(app_dir, &ProcfsInspector),
        TaskControlCommand::Kill { task_id } => kill_task(
            app_dir,
            &task_id,
            &ProcfsInspector,
            &mut HostTaskSignaler,
            thread::sleep,
        ),
    }
}

fn render_ps(app_dir: &Path, inspector: &impl ProcessInspector) -> Result<String> {
    let rows = list_records(app_dir)?
        .into_iter()
        .map(|record| {
            let status = inspector.status(&record.process);
            TaskRow { record, status }
        })
        .collect::<Vec<_>>();

    Ok(render_task_table(&rows))
}

fn kill_task(
    app_dir: &Path,
    task_id: &str,
    inspector: &impl ProcessInspector,
    signaler: &mut impl TaskSignaler,
    sleep: impl FnOnce(Duration),
) -> Result<String> {
    let matches = list_records(app_dir)?
        .into_iter()
        .filter(|record| record.task_id == task_id)
        .collect::<Vec<_>>();

    match matches.as_slice() {
        [] => bail!("no active loftd task with id '{task_id}'"),
        [record] => kill_record(record, inspector, signaler, sleep),
        _ => bail!("multiple active loftd task records matched id '{task_id}'; refusing to kill"),
    }
}

fn kill_record(
    record: &ActiveTaskRecord,
    inspector: &impl ProcessInspector,
    signaler: &mut impl TaskSignaler,
    sleep: impl FnOnce(Duration),
) -> Result<String> {
    ensure_running(record, inspector)?;
    if ignore_missing_process_group(
        signaler.signal_process_group(record.process.pgid, libc::SIGTERM),
    )?
    .is_missing()
    {
        return finish_kill_success(
            record,
            format!(
                "loftd task '{}' already exited before SIGTERM (session {}, process group {})\n",
                record.task_id, record.process.sid, record.process.pgid
            ),
        );
    }
    sleep(TERM_GRACE);
    if inspector.status(&record.process) == ActiveTaskStatus::Running {
        if inspector.status(&record.process) != ActiveTaskStatus::Running {
            return finish_kill_success(
                record,
                format!(
                    "sent SIGTERM to loftd task '{}' (session {}, process group {})\n",
                    record.task_id, record.process.sid, record.process.pgid
                ),
            );
        }
        ignore_missing_process_group(
            signaler.signal_process_group(record.process.pgid, libc::SIGKILL),
        )?;
        return finish_kill_success(
            record,
            format!(
                "sent SIGTERM then SIGKILL to loftd task '{}' (session {}, process group {})\n",
                record.task_id, record.process.sid, record.process.pgid
            ),
        );
    }
    finish_kill_success(
        record,
        format!(
            "sent SIGTERM to loftd task '{}' (session {}, process group {})\n",
            record.task_id, record.process.sid, record.process.pgid
        ),
    )
}

fn finish_kill_success(record: &ActiveTaskRecord, message: String) -> Result<String> {
    remove_active_task(&record.task_dir)?;
    Ok(message)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SignalOutcome {
    Sent,
    Missing,
}

impl SignalOutcome {
    fn is_missing(self) -> bool {
        self == Self::Missing
    }
}

fn ignore_missing_process_group(result: Result<()>) -> Result<SignalOutcome> {
    match result {
        Ok(()) => Ok(SignalOutcome::Sent),
        Err(err) if is_esrch(&err) => Ok(SignalOutcome::Missing),
        Err(err) => Err(err),
    }
}

fn is_esrch(err: &anyhow::Error) -> bool {
    err.chain()
        .filter_map(|cause| cause.downcast_ref::<std::io::Error>())
        .any(|io| io.raw_os_error() == Some(libc::ESRCH))
}

fn ensure_running(record: &ActiveTaskRecord, inspector: &impl ProcessInspector) -> Result<()> {
    match inspector.status(&record.process) {
        ActiveTaskStatus::Running => Ok(()),
        ActiveTaskStatus::Stale => bail!("task '{}' is stale; refusing to signal", record.task_id),
        ActiveTaskStatus::PidReused => bail!(
            "task '{}' process id was reused; refusing to signal",
            record.task_id
        ),
        ActiveTaskStatus::Unreadable => bail!(
            "task '{}' process identity is unreadable; refusing to signal",
            record.task_id
        ),
    }
}

fn list_records(app_dir: &Path) -> Result<Vec<ActiveTaskRecord>> {
    let mut records = Vec::new();
    if !app_dir.exists() {
        return Ok(records);
    }

    for workspace_entry in fs::read_dir(app_dir).with_context(|| {
        format!(
            "failed to read loftd state directory '{}'",
            app_dir.display()
        )
    })? {
        let workspace_entry = workspace_entry?;
        let workspace_path = workspace_entry.path();
        if !workspace_path.is_dir() || is_non_workspace_state_dir(workspace_path.file_name()) {
            continue;
        }
        let tasks_dir = workspace_path.join("tasks");
        if !tasks_dir.is_dir() {
            continue;
        }
        for task_entry in fs::read_dir(&tasks_dir)
            .with_context(|| format!("failed to read tasks directory '{}'", tasks_dir.display()))?
        {
            let task_entry = task_entry?;
            let task_path = task_entry.path();
            if !task_path.is_dir() {
                continue;
            }
            let record_path = active_record_path(&task_path);
            if record_path.is_file() {
                records.push(read_active_task_record(&record_path)?);
            }
        }
    }
    records.sort_by(|left, right| left.task_id.cmp(&right.task_id));
    Ok(records)
}

fn is_non_workspace_state_dir(name: Option<&OsStr>) -> bool {
    matches!(
        name.and_then(OsStr::to_str),
        Some("microvm") | Some("sccache")
    )
}

fn render_task_table(rows: &[TaskRow]) -> String {
    let mut lines = vec![format!(
        "{:<36} {:<10} {:>7} {:>7} {:>12}  {:<24} WORKSPACE",
        "TASK ID", "STATUS", "PID", "SID", "STARTED", "IMAGE"
    )];
    for row in rows {
        let digest = row
            .record
            .image_digest
            .as_deref()
            .map(|digest| format!("@{digest}"))
            .unwrap_or_default();
        lines.push(format!(
            "{:<36} {:<10} {:>7} {:>7} {:>12}  {:<24} {}",
            truncate(&row.record.task_id, 36),
            row.status.as_str(),
            row.record.process.pid,
            row.record.process.sid,
            row.record.started_at_unix_secs,
            truncate(&format!("{}{}", row.record.image_reference, digest), 24),
            row.record.workspace_slug,
        ));
    }
    lines.push(String::new());
    lines.join("\n")
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TaskRow {
    record: ActiveTaskRecord,
    status: ActiveTaskStatus,
}

fn truncate(value: &str, width: usize) -> String {
    if value.chars().count() <= width {
        return value.to_owned();
    }
    let mut output = value
        .chars()
        .take(width.saturating_sub(1))
        .collect::<String>();
    output.push('…');
    output
}

fn active_record_path(task_dir: &Path) -> PathBuf {
    task_dir.join(ACTIVE_RECORD_FILE)
}

fn write_active_task_record(record: &ActiveTaskRecord) -> Result<()> {
    fs::create_dir_all(&record.task_dir)
        .with_context(|| format!("failed to create '{}'", record.task_dir.display()))?;
    let path = active_record_path(&record.task_dir);
    fs::write(&path, encode_record(record))
        .with_context(|| format!("failed to write '{}'", path.display()))
}

fn read_active_task_record(path: &Path) -> Result<ActiveTaskRecord> {
    let contents = fs::read_to_string(path)
        .with_context(|| format!("failed to read active task record '{}'", path.display()))?;
    decode_record(&contents).with_context(|| format!("failed to parse '{}'", path.display()))
}

fn encode_record(record: &ActiveTaskRecord) -> String {
    let mut output = String::new();
    push_kv(&mut output, "version", "1");
    push_kv(&mut output, "task_id", &record.task_id);
    push_kv(&mut output, "workspace_slug", &record.workspace_slug);
    push_kv(
        &mut output,
        "workspace_dir",
        &record.workspace_dir.display().to_string(),
    );
    push_kv(
        &mut output,
        "task_dir",
        &record.task_dir.display().to_string(),
    );
    push_kv(&mut output, "image_reference", &record.image_reference);
    if let Some(digest) = &record.image_digest {
        push_kv(&mut output, "image_digest", digest);
    }
    push_kv(
        &mut output,
        "started_at_unix_secs",
        &record.started_at_unix_secs.to_string(),
    );
    push_kv(&mut output, "pid", &record.process.pid.to_string());
    push_kv(&mut output, "pgid", &record.process.pgid.to_string());
    push_kv(&mut output, "sid", &record.process.sid.to_string());
    push_kv(
        &mut output,
        "proc_start_time_ticks",
        &record.process.proc_start_time_ticks.to_string(),
    );
    push_kv(&mut output, "boot_id", &record.process.boot_id);
    output
}

fn push_kv(output: &mut String, key: &str, value: &str) {
    output.push_str(key);
    output.push('=');
    output.push_str(value.replace('\n', " ").trim());
    output.push('\n');
}

fn decode_record(contents: &str) -> Result<ActiveTaskRecord> {
    let mut map = std::collections::BTreeMap::new();
    for line in contents.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            bail!("active task record line is missing '=': {line}");
        };
        map.insert(key.to_owned(), value.to_owned());
    }
    if required(&map, "version")? != "1" {
        bail!("unsupported active task record version");
    }
    Ok(ActiveTaskRecord {
        task_id: required(&map, "task_id")?.to_owned(),
        workspace_slug: required(&map, "workspace_slug")?.to_owned(),
        workspace_dir: PathBuf::from(required(&map, "workspace_dir")?),
        task_dir: PathBuf::from(required(&map, "task_dir")?),
        image_reference: required(&map, "image_reference")?.to_owned(),
        image_digest: map.get("image_digest").cloned(),
        started_at_unix_secs: parse_required(&map, "started_at_unix_secs")?,
        process: ProcessIdentity {
            pid: parse_required(&map, "pid")?,
            pgid: parse_required(&map, "pgid")?,
            sid: parse_required(&map, "sid")?,
            proc_start_time_ticks: parse_required(&map, "proc_start_time_ticks")?,
            boot_id: required(&map, "boot_id")?.to_owned(),
        },
    })
}

fn required<'a>(map: &'a std::collections::BTreeMap<String, String>, key: &str) -> Result<&'a str> {
    map.get(key)
        .map(String::as_str)
        .ok_or_else(|| anyhow!("active task record is missing '{key}'"))
}

fn parse_required<T>(map: &std::collections::BTreeMap<String, String>, key: &str) -> Result<T>
where
    T: std::str::FromStr,
    T::Err: std::error::Error + Send + Sync + 'static,
{
    required(map, key)?
        .parse::<T>()
        .with_context(|| format!("active task record field '{key}' has invalid value"))
}

fn read_process_identity(
    proc_root: &Path,
    boot_id_path: &Path,
    pid: u32,
    pgid: u32,
    sid: u32,
) -> Result<ProcessIdentity> {
    Ok(ProcessIdentity {
        pid,
        pgid,
        sid,
        proc_start_time_ticks: read_proc_start_time_ticks(proc_root, pid)?,
        boot_id: read_boot_id(boot_id_path)?,
    })
}

fn inspect_process_identity(
    proc_root: &Path,
    boot_id_path: &Path,
    identity: &ProcessIdentity,
) -> Result<ActiveTaskStatus> {
    if read_boot_id(boot_id_path)? != identity.boot_id {
        return Ok(ActiveTaskStatus::PidReused);
    }
    if !proc_root.join(identity.pid.to_string()).exists() {
        return Ok(ActiveTaskStatus::Stale);
    }
    let start_time = read_proc_start_time_ticks(proc_root, identity.pid)?;
    if start_time != identity.proc_start_time_ticks {
        return Ok(ActiveTaskStatus::PidReused);
    }
    let actual_pgid = unsafe { libc::getpgid(identity.pid as libc::pid_t) };
    if actual_pgid < 0 {
        return Ok(ActiveTaskStatus::Stale);
    }
    if u32::try_from(actual_pgid).ok() != Some(identity.pgid) {
        return Ok(ActiveTaskStatus::PidReused);
    }
    let actual_sid = unsafe { libc::getsid(identity.pid as libc::pid_t) };
    if actual_sid < 0 {
        return Ok(ActiveTaskStatus::Stale);
    }
    if u32::try_from(actual_sid).ok() != Some(identity.sid) {
        return Ok(ActiveTaskStatus::PidReused);
    }
    Ok(ActiveTaskStatus::Running)
}

fn read_proc_start_time_ticks(proc_root: &Path, pid: u32) -> Result<u64> {
    let stat_path = proc_root.join(pid.to_string()).join("stat");
    let stat = fs::read_to_string(&stat_path)
        .with_context(|| format!("failed to read '{}'", stat_path.display()))?;
    parse_proc_stat_start_time(&stat)
}

fn parse_proc_stat_start_time(stat: &str) -> Result<u64> {
    let after_comm = stat
        .rsplit_once(") ")
        .map(|(_, rest)| rest)
        .ok_or_else(|| anyhow!("/proc stat line is missing process command terminator"))?;
    let fields = after_comm.split_whitespace().collect::<Vec<_>>();
    let start_time = fields
        .get(19)
        .ok_or_else(|| anyhow!("/proc stat line is missing starttime field"))?;
    start_time
        .parse::<u64>()
        .context("/proc stat starttime is not an integer")
}

fn read_boot_id(path: &Path) -> Result<String> {
    Ok(fs::read_to_string(path)
        .with_context(|| format!("failed to read '{}'", path.display()))?
        .trim()
        .to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[derive(Debug)]
    struct StaticInspector {
        statuses: std::cell::RefCell<Vec<ActiveTaskStatus>>,
    }

    impl StaticInspector {
        fn new(statuses: Vec<ActiveTaskStatus>) -> Self {
            Self {
                statuses: std::cell::RefCell::new(statuses),
            }
        }
    }

    impl ProcessInspector for StaticInspector {
        fn status(&self, _identity: &ProcessIdentity) -> ActiveTaskStatus {
            self.statuses.borrow_mut().remove(0)
        }
    }

    #[derive(Debug, Default)]
    struct RecordingSignaler {
        signals: Vec<(u32, i32)>,
        fail_on_signal: Option<i32>,
    }

    impl TaskSignaler for RecordingSignaler {
        fn signal_process_group(&mut self, pgid: u32, signal: i32) -> Result<()> {
            self.signals.push((pgid, signal));
            if self.fail_on_signal == Some(signal) {
                return Err(anyhow!(std::io::Error::from_raw_os_error(libc::ESRCH)))
                    .with_context(|| format!("failed to send signal {signal}"));
            }
            Ok(())
        }
    }

    fn record(task_id: &str, task_dir: PathBuf) -> ActiveTaskRecord {
        ActiveTaskRecord {
            task_id: task_id.to_owned(),
            workspace_slug: "workspace-a".to_owned(),
            workspace_dir: PathBuf::from("/src/workspace-a"),
            task_dir,
            image_reference: "localhost/loftd:latest".to_owned(),
            image_digest: Some("sha256:abc".to_owned()),
            started_at_unix_secs: 12,
            process: ProcessIdentity {
                pid: 123,
                pgid: 123,
                sid: 123,
                proc_start_time_ticks: 456,
                boot_id: "boot".to_owned(),
            },
        }
    }

    #[test]
    fn active_record_round_trips_without_external_format_dependency() {
        let dir = tempfile::tempdir().expect("tempdir");
        let task_dir = dir.path().join("workspace-a/tasks/task-a");
        let original = record("task-a", task_dir.clone());

        write_active_task_record(&original).expect("write record");
        let decoded = read_active_task_record(&active_record_path(&task_dir)).expect("read record");

        assert_eq!(decoded, original);
    }

    #[test]
    fn list_records_scans_workspace_tasks_and_skips_app_support_dirs() {
        let dir = tempfile::tempdir().expect("tempdir");
        let app_dir = dir.path().join("loftd");
        let task_dir = app_dir.join("workspace-a/tasks/task-a");
        write_active_task_record(&record("task-a", task_dir)).expect("write task record");
        fs::create_dir_all(app_dir.join("microvm/tasks/ignored")).expect("support dir");
        fs::create_dir_all(app_dir.join("sccache/tasks/ignored")).expect("support dir");

        let records = list_records(&app_dir).expect("list records");

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].task_id, "task-a");
    }

    #[test]
    fn ps_renders_human_readable_table_with_status() {
        let dir = tempfile::tempdir().expect("tempdir");
        let app_dir = dir.path().join("loftd");
        let task_dir = app_dir.join("workspace-a/tasks/task-a");
        write_active_task_record(&record("task-a", task_dir)).expect("write task record");

        let output = render_ps(
            &app_dir,
            &StaticInspector::new(vec![ActiveTaskStatus::Running]),
        )
        .expect("render ps");

        assert!(output.contains("TASK ID"));
        assert!(output.contains("running"));
        assert!(output.contains("workspace-a"));
    }

    #[test]
    fn kill_sends_term_only_when_task_exits_during_grace_period() {
        let dir = tempfile::tempdir().expect("tempdir");
        let app_dir = dir.path().join("loftd");
        let task_dir = app_dir.join("workspace-a/tasks/task-a");
        let record_path = active_record_path(&task_dir);
        write_active_task_record(&record("task-a", task_dir)).expect("write task record");
        let inspector =
            StaticInspector::new(vec![ActiveTaskStatus::Running, ActiveTaskStatus::Stale]);
        let mut signaler = RecordingSignaler::default();
        let slept = Cell::new(false);

        let output = kill_task(&app_dir, "task-a", &inspector, &mut signaler, |duration| {
            assert_eq!(duration, TERM_GRACE);
            slept.set(true);
        })
        .expect("kill task");

        assert!(slept.get());
        assert!(output.contains("SIGTERM"));
        assert_eq!(signaler.signals, vec![(123, libc::SIGTERM)]);
        assert!(!record_path.exists());
    }

    #[test]
    fn kill_escalates_to_sigkill_when_task_remains_running() {
        let dir = tempfile::tempdir().expect("tempdir");
        let app_dir = dir.path().join("loftd");
        let task_dir = app_dir.join("workspace-a/tasks/task-a");
        let record_path = active_record_path(&task_dir);
        write_active_task_record(&record("task-a", task_dir)).expect("write task record");
        let inspector = StaticInspector::new(vec![
            ActiveTaskStatus::Running,
            ActiveTaskStatus::Running,
            ActiveTaskStatus::Running,
        ]);
        let mut signaler = RecordingSignaler::default();

        let output =
            kill_task(&app_dir, "task-a", &inspector, &mut signaler, |_| {}).expect("kill task");

        assert!(output.contains("SIGKILL"));
        assert_eq!(
            signaler.signals,
            vec![(123, libc::SIGTERM), (123, libc::SIGKILL)]
        );
        assert!(!record_path.exists());
    }

    #[test]
    fn kill_cleans_record_when_task_exits_between_post_grace_check_and_sigkill() {
        let dir = tempfile::tempdir().expect("tempdir");
        let app_dir = dir.path().join("loftd");
        let task_dir = app_dir.join("workspace-a/tasks/task-a");
        let record_path = active_record_path(&task_dir);
        write_active_task_record(&record("task-a", task_dir)).expect("write task record");
        let inspector = StaticInspector::new(vec![
            ActiveTaskStatus::Running,
            ActiveTaskStatus::Running,
            ActiveTaskStatus::Stale,
        ]);
        let mut signaler = RecordingSignaler::default();

        let output =
            kill_task(&app_dir, "task-a", &inspector, &mut signaler, |_| {}).expect("kill task");

        assert!(output.contains("SIGTERM"));
        assert!(!output.contains("SIGKILL"));
        assert_eq!(signaler.signals, vec![(123, libc::SIGTERM)]);
        assert!(!record_path.exists());
    }

    #[test]
    fn kill_cleans_record_when_sigkill_loses_exit_race() {
        let dir = tempfile::tempdir().expect("tempdir");
        let app_dir = dir.path().join("loftd");
        let task_dir = app_dir.join("workspace-a/tasks/task-a");
        let record_path = active_record_path(&task_dir);
        write_active_task_record(&record("task-a", task_dir)).expect("write task record");
        let inspector = StaticInspector::new(vec![
            ActiveTaskStatus::Running,
            ActiveTaskStatus::Running,
            ActiveTaskStatus::Running,
        ]);
        let mut signaler = RecordingSignaler {
            signals: Vec::new(),
            fail_on_signal: Some(libc::SIGKILL),
        };

        let output =
            kill_task(&app_dir, "task-a", &inspector, &mut signaler, |_| {}).expect("kill task");

        assert!(output.contains("SIGKILL"));
        assert_eq!(
            signaler.signals,
            vec![(123, libc::SIGTERM), (123, libc::SIGKILL)]
        );
        assert!(!record_path.exists());
    }

    #[test]
    fn kill_cleans_record_when_sigterm_loses_exit_race() {
        let dir = tempfile::tempdir().expect("tempdir");
        let app_dir = dir.path().join("loftd");
        let task_dir = app_dir.join("workspace-a/tasks/task-a");
        let record_path = active_record_path(&task_dir);
        write_active_task_record(&record("task-a", task_dir)).expect("write task record");
        let inspector = StaticInspector::new(vec![ActiveTaskStatus::Running]);
        let mut signaler = RecordingSignaler {
            signals: Vec::new(),
            fail_on_signal: Some(libc::SIGTERM),
        };

        let output =
            kill_task(&app_dir, "task-a", &inspector, &mut signaler, |_| {}).expect("kill task");

        assert!(output.contains("already exited"));
        assert_eq!(signaler.signals, vec![(123, libc::SIGTERM)]);
        assert!(!record_path.exists());
    }

    #[test]
    fn kill_refuses_stale_or_reused_task_without_signaling() {
        let dir = tempfile::tempdir().expect("tempdir");
        let app_dir = dir.path().join("loftd");
        let task_dir = app_dir.join("workspace-a/tasks/task-a");
        write_active_task_record(&record("task-a", task_dir)).expect("write task record");
        let inspector = StaticInspector::new(vec![ActiveTaskStatus::PidReused]);
        let mut signaler = RecordingSignaler::default();

        let err = kill_task(&app_dir, "task-a", &inspector, &mut signaler, |_| {})
            .expect_err("pid reuse should refuse kill");

        assert!(format!("{err:#}").contains("reused"));
        assert!(signaler.signals.is_empty());
    }

    #[test]
    fn proc_stat_parser_handles_process_names_with_spaces() {
        let stat = "123 (name with spaces) S 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 98765 20";

        assert_eq!(parse_proc_stat_start_time(stat).unwrap(), 98765);
    }
}
