//! Classification of a guest death from the captured guest console.
//!
//! The guest kernel explains its OOM decisions on the serial console, which
//! cang captures into the managed helper's stderr log. Without reading it, a
//! guest that dies under memory pressure looks exactly like a task that
//! finished, and the microVM reboots without a word.

use std::fmt::Write as _;

/// Kernel text this module reads (`mm/oom_kill.c`):
///
/// ```text
/// oom-kill:constraint=CONSTRAINT_NONE,nodemask=(null),cpuset=/,mems_allowed=0,global_oom,task=python3,pid=1234,uid=1000
/// Out of memory: Killed process 1234 (python3) total-vm:5242880kB, anon-rss:4194304kB, file-rss:1024kB, shmem-rss:0kB, UID:1000 pgtables:8192kB oom_score_adj:0
/// Memory cgroup out of memory: Killed process 1234 (python3) ...
/// Out of memory and no killable processes...
/// ```
const KILLED_PROCESS_MARKER: &str = "Killed process ";
const OOM_KILL_MARKER: &str = "oom-kill:";
const NO_KILLABLE_PROCESSES_MARKER: &str = "Out of memory and no killable processes";

/// Why a guest session ended, as far as the captured console shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum GuestDeathCause {
    /// The guest kernel's OOM killer killed a task.
    OomKilled {
        task: String,
        pid: Option<u32>,
        anon_rss_kib: Option<u64>,
        kills: u32,
    },
    /// The guest kernel had no task it was allowed to kill left and panicked.
    NoKillableProcesses,
    /// The console shows nothing this module recognises.
    Unknown,
}

impl GuestDeathCause {
    /// Reads the OOM evidence out of captured guest console output.
    pub(super) fn diagnose(console: &str) -> Self {
        let mut kills = Vec::new();
        let mut summary = None;
        let mut saw_no_killable_processes = false;
        for line in console.lines() {
            if line.contains(NO_KILLABLE_PROCESSES_MARKER) {
                saw_no_killable_processes = true;
            }
            if let Some(kill) = parse_killed_process(line) {
                kills.push(kill);
                continue;
            }
            if let Some(parsed) = parse_oom_kill_summary(line) {
                summary = Some(parsed);
            }
        }
        if saw_no_killable_processes {
            return Self::NoKillableProcesses;
        }
        if let Some(kill) = kills.last() {
            return Self::OomKilled {
                task: kill.task.clone(),
                pid: kill.pid,
                anon_rss_kib: kill.anon_rss_kib,
                kills: u32::try_from(kills.len()).unwrap_or(u32::MAX),
            };
        }
        // A truncated console may keep only the summary line.
        if let Some(summary) = summary {
            return Self::OomKilled {
                task: summary.task.unwrap_or_else(|| "an unknown task".to_owned()),
                pid: summary.pid,
                anon_rss_kib: None,
                kills: 1,
            };
        }
        Self::Unknown
    }

    /// A one-line explanation, or `None` when the console showed no cause.
    pub(super) fn describe(&self) -> Option<String> {
        match self {
            Self::OomKilled {
                task,
                pid,
                anon_rss_kib,
                kills,
            } => {
                let mut out = format!("guest kernel OOM-killed {task}");
                if let Some(pid) = pid {
                    let _ = write!(out, " (pid {pid})");
                }
                if let Some(anon_rss_kib) = anon_rss_kib {
                    let _ = write!(out, ", anon-rss {anon_rss_kib} kB");
                }
                if *kills > 1 {
                    let _ = write!(out, "; {kills} OOM kills in this session");
                }
                Some(out)
            }
            Self::NoKillableProcesses => Some(
                "guest kernel found no killable task and panicked (system is deadlocked on memory)"
                    .to_owned(),
            ),
            Self::Unknown => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct OomKill {
    task: String,
    pid: Option<u32>,
    anon_rss_kib: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct OomKillSummary {
    task: Option<String>,
    pid: Option<u32>,
}

/// `...: Killed process <pid> (<comm>) ... anon-rss:<kB>kB, ...`
fn parse_killed_process(line: &str) -> Option<OomKill> {
    let tail = line.split_once(KILLED_PROCESS_MARKER)?.1;
    let (pid_text, rest) = tail.split_once(' ')?;
    let pid = pid_text.trim().parse::<u32>().ok();
    let rest = rest.trim_start().strip_prefix('(')?;
    let (task, after_task) = rest.split_once(')')?;
    let anon_rss_kib = after_task
        .split_once("anon-rss:")
        .and_then(|(_, tail)| tail.split("kB").next())
        .and_then(|value| value.trim().parse::<u64>().ok());
    Some(OomKill {
        task: task.to_owned(),
        pid,
        anon_rss_kib,
    })
}

/// `oom-kill:constraint=...,task=<comm>,pid=<pid>,uid=<uid>`
fn parse_oom_kill_summary(line: &str) -> Option<OomKillSummary> {
    let tail = line.split_once(OOM_KILL_MARKER)?.1;
    let mut task = None;
    let mut pid = None;
    for field in tail.split(',') {
        match field.split_once('=') {
            Some(("task", value)) => task = Some(value.trim().to_owned()),
            Some(("pid", value)) => pid = value.trim().parse::<u32>().ok(),
            _ => {}
        }
    }
    if task.is_none() && pid.is_none() {
        return None;
    }
    Some(OomKillSummary { task, pid })
}

#[cfg(test)]
#[path = "guest_death_tests.rs"]
mod tests;
