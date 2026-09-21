use super::*;

/// Console text as the guest kernel prints it for a global OOM kill
/// (`mm/oom_kill.c` `dump_header` + `oom_kill_process`).
const GLOBAL_OOM_CONSOLE: &str = "\
[    0.000000] Linux version 6.12.91-hardened1
python3 invoked oom-killer: gfp_mask=0x140cca(GFP_HIGHUSER_MOVABLE|__GFP_COMP), order=0, oom_score_adj=0
oom-kill:constraint=CONSTRAINT_NONE,nodemask=(null),cpuset=/,mems_allowed=0,global_oom,task=python3,pid=1234,uid=1000
Out of memory: Killed process 1234 (python3) total-vm:5242880kB, anon-rss:4194304kB, file-rss:1024kB, shmem-rss:0kB, UID:1000 pgtables:8192kB oom_score_adj:0
";

#[test]
fn diagnose_recognizes_a_global_oom_kill() {
    let cause = GuestDeathCause::diagnose(GLOBAL_OOM_CONSOLE);

    assert_eq!(
        cause,
        GuestDeathCause::OomKilled {
            task: "python3".to_owned(),
            pid: Some(1234),
            anon_rss_kib: Some(4_194_304),
            kills: 1,
        }
    );
    assert_eq!(
        cause.describe().unwrap(),
        "guest kernel OOM-killed python3 (pid 1234), anon-rss 4194304 kB"
    );
}

#[test]
fn diagnose_recognizes_a_memory_cgroup_oom_kill() {
    let console = "Memory cgroup out of memory: Killed process 77 (cargo) total-vm:1024kB, anon-rss:2048kB, file-rss:0kB, shmem-rss:0kB, UID:1000 pgtables:64kB oom_score_adj:0\n";

    assert_eq!(
        GuestDeathCause::diagnose(console),
        GuestDeathCause::OomKilled {
            task: "cargo".to_owned(),
            pid: Some(77),
            anon_rss_kib: Some(2048),
            kills: 1,
        }
    );
}

#[test]
fn diagnose_prefers_the_no_killable_processes_panic_over_earlier_kills() {
    let console = format!(
        "{GLOBAL_OOM_CONSOLE}Out of memory and no killable processes...\nKernel panic - not syncing: System is deadlocked on memory\n"
    );

    let cause = GuestDeathCause::diagnose(&console);

    assert_eq!(cause, GuestDeathCause::NoKillableProcesses);
    assert!(cause.describe().unwrap().contains("no killable task"));
}

#[test]
fn diagnose_falls_back_to_the_oom_kill_summary_line() {
    let console = "oom-kill:constraint=CONSTRAINT_NONE,nodemask=(null),cpuset=/,mems_allowed=0,global_oom,task=node,pid=4321,uid=1000\n";

    let cause = GuestDeathCause::diagnose(console);

    assert_eq!(
        cause,
        GuestDeathCause::OomKilled {
            task: "node".to_owned(),
            pid: Some(4321),
            anon_rss_kib: None,
            kills: 1,
        }
    );
}

#[test]
fn diagnose_counts_every_kill_in_the_console() {
    let console = "Out of memory: Killed process 10 (a) total-vm:1kB, anon-rss:1kB, file-rss:0kB, shmem-rss:0kB, UID:0 pgtables:1kB oom_score_adj:0\n\
                   Out of memory: Killed process 20 (b) total-vm:1kB, anon-rss:2kB, file-rss:0kB, shmem-rss:0kB, UID:0 pgtables:1kB oom_score_adj:0\n";

    let cause = GuestDeathCause::diagnose(console);

    assert_eq!(
        cause,
        GuestDeathCause::OomKilled {
            task: "b".to_owned(),
            pid: Some(20),
            anon_rss_kib: Some(2),
            kills: 2,
        }
    );
    assert!(cause.describe().unwrap().contains("2 OOM kills"));
}

#[test]
fn diagnose_tolerates_a_kill_line_without_the_anon_rss_field() {
    let console = "Out of memory: Killed process 99 (sh)\n";

    assert_eq!(
        GuestDeathCause::diagnose(console),
        GuestDeathCause::OomKilled {
            task: "sh".to_owned(),
            pid: Some(99),
            anon_rss_kib: None,
            kills: 1,
        }
    );
}

#[test]
fn diagnose_reports_nothing_for_an_ordinary_console() {
    let console = "[    0.000000] Linux version 6.12.91-hardened1\nloftd-guest-init: prep complete\nfs.file-max=524288\n";

    let cause = GuestDeathCause::diagnose(console);

    assert_eq!(cause, GuestDeathCause::Unknown);
    assert_eq!(cause.describe(), None);
}
