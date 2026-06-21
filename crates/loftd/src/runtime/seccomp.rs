//! Host-side seccomp audit, synthesis, and enforcement support.

use anyhow::{Context, Result, anyhow, bail};
use clap::Subcommand;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs;
use std::io::{BufRead, BufReader, Cursor, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) enum SeccompMode {
    #[default]
    Off,
    Audit(AuditMode),
    Enforce {
        policy_path: PathBuf,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AuditMode {
    Full {
        trace_path: PathBuf,
    },
    Gap {
        baseline_policy_path: PathBuf,
        trace_path: PathBuf,
    },
}

impl AuditMode {
    pub(crate) fn trace_path(&self) -> &Path {
        match self {
            Self::Full { trace_path } | Self::Gap { trace_path, .. } => trace_path,
        }
    }

    pub(crate) fn baseline_policy_path(&self) -> Option<&Path> {
        match self {
            Self::Gap {
                baseline_policy_path,
                ..
            } => Some(baseline_policy_path),
            Self::Full { .. } => None,
        }
    }
}

impl SeccompMode {
    pub(crate) fn as_config_value(&self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Audit(_) => "audit",
            Self::Enforce { .. } => "enforce",
        }
    }

    pub(crate) fn parse_config_value(
        mode: Option<&str>,
        audit_trace_path: Option<&str>,
        audit_baseline_policy_path: Option<&str>,
        enforce_policy_path: Option<&str>,
    ) -> Result<Self> {
        match mode.unwrap_or("off") {
            "off" => {
                if audit_trace_path.is_some()
                    || audit_baseline_policy_path.is_some()
                    || enforce_policy_path.is_some()
                {
                    bail!("loftd launch config seccomp off mode rejects seccomp path fields");
                }
                Ok(Self::Off)
            }
            "audit" => {
                if enforce_policy_path.is_some() {
                    bail!("loftd launch config audit mode rejects seccomp.enforce_policy_path");
                }
                let trace_path = PathBuf::from(audit_trace_path.ok_or_else(|| {
                    anyhow!("loftd launch config seccomp.audit_trace_path is required")
                })?);
                Ok(Self::Audit(match audit_baseline_policy_path {
                    Some(path) => AuditMode::Gap {
                        baseline_policy_path: PathBuf::from(path),
                        trace_path,
                    },
                    None => AuditMode::Full { trace_path },
                }))
            }
            "enforce" => {
                if audit_trace_path.is_some() || audit_baseline_policy_path.is_some() {
                    bail!("loftd launch config enforce mode rejects seccomp audit path fields");
                }
                Ok(Self::Enforce {
                    policy_path: PathBuf::from(enforce_policy_path.ok_or_else(|| {
                        anyhow!("loftd launch config seccomp.enforce_policy_path is required")
                    })?),
                })
            }
            _ => bail!("loftd launch config seccomp.mode is invalid"),
        }
    }

    pub(crate) fn audit_trace_path(&self) -> Option<&Path> {
        match self {
            Self::Audit(mode) => Some(mode.trace_path()),
            Self::Off | Self::Enforce { .. } => None,
        }
    }

    pub(crate) fn audit_baseline_policy_path(&self) -> Option<&Path> {
        match self {
            Self::Audit(mode) => mode.baseline_policy_path(),
            Self::Off | Self::Enforce { .. } => None,
        }
    }

    pub(crate) fn enforce_policy_path(&self) -> Option<&Path> {
        match self {
            Self::Enforce { policy_path } => Some(policy_path),
            Self::Off | Self::Audit(_) => None,
        }
    }
}

#[derive(Debug, Clone, Subcommand, PartialEq, Eq)]
pub(crate) enum SeccompCommand {
    #[command(
        name = "synthesize",
        about = "Synthesize a seccompiler allowlist policy from a loftd seccomp audit trace"
    )]
    Synthesize {
        #[arg(long = "input", value_name = "TRACE_JSONL")]
        input: PathBuf,
        #[arg(long = "output", value_name = "POLICY_JSON")]
        output: PathBuf,
    },

    #[command(
        name = "extend",
        about = "Add missing audited syscalls to an existing seccompiler allowlist policy"
    )]
    Extend {
        #[arg(long = "policy", value_name = "BASELINE_POLICY_JSON")]
        policy: PathBuf,
        #[arg(long = "trace", value_name = "MISSING_TRACE_JSONL")]
        trace: PathBuf,
        #[arg(long = "output", value_name = "UPDATED_POLICY_JSON")]
        output: PathBuf,
    },
}

pub(crate) fn run_seccomp_command(command: SeccompCommand) -> Result<String> {
    match command {
        SeccompCommand::Synthesize { input, output } => {
            synthesize_policy(&input, &output)?;
            Ok(format!(
                "wrote seccomp policy '{}' from '{}'\n",
                output.display(),
                input.display()
            ))
        }
        SeccompCommand::Extend {
            policy,
            trace,
            output,
        } => {
            extend_policy(&policy, &trace, &output)?;
            Ok(format!(
                "wrote extended seccomp policy '{}' from policy '{}' and trace '{}'\n",
                output.display(),
                policy.display(),
                trace.display()
            ))
        }
    }
}

pub(crate) fn raw_strace_path(trace_path: &Path) -> PathBuf {
    let mut name = trace_path
        .file_name()
        .map(|name| name.to_os_string())
        .unwrap_or_else(|| "loftd-seccomp-trace".into());
    name.push(".strace");
    trace_path.with_file_name(name)
}

pub(crate) fn ptrace_failure_hint() -> &'static str {
    "seccomp audit mode uses strace/ptrace on the loftd VM worker only; normal child tracing should work with `kernel.yama.ptrace_scope=1`, but hosts that disable ptrace entirely must allow ptrace for the audit run"
}

pub(crate) fn prepare_audit_trace_target(trace_path: &Path) -> Result<()> {
    if let Some(parent) = trace_path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!("failed to create seccomp trace dir '{}'", parent.display())
        })?;
    }
    let raw_path = raw_strace_path(trace_path);
    if let Some(parent) = raw_path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create raw seccomp trace dir '{}'",
                parent.display()
            )
        })?;
    }
    Ok(())
}

pub(crate) fn finalize_audit_trace(trace_path: &Path) -> Result<()> {
    let raw_path = raw_strace_path(trace_path);
    if let Some(parent) = trace_path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!("failed to create seccomp trace dir '{}'", parent.display())
        })?;
    }
    let temp_path = finalized_trace_temp_path(trace_path);
    match write_finalized_audit_trace(&raw_path, &temp_path) {
        Ok(()) => fs::rename(&temp_path, trace_path).with_context(|| {
            format!(
                "failed to publish finalized seccomp trace '{}' from '{}'",
                trace_path.display(),
                temp_path.display()
            )
        }),
        Err(err) => {
            let _ = fs::remove_file(&temp_path);
            Err(err)
        }
    }
}

fn write_finalized_audit_trace(raw_path: &Path, output_path: &Path) -> Result<()> {
    let raw = fs::File::open(raw_path)
        .with_context(|| format!("failed to open raw strace log '{}'", raw_path.display()))?;
    let mut out = fs::File::create(output_path)
        .with_context(|| format!("failed to create seccomp trace '{}'", output_path.display()))?;
    for line in BufReader::new(raw).lines() {
        let line = line.with_context(|| format!("failed to read '{}'", raw_path.display()))?;
        let Some(syscall) = syscall_from_strace_line(&line) else {
            continue;
        };
        let record = TraceRecord { syscall, raw: line };
        serde_json::to_writer(&mut out, &record)
            .context("failed to encode seccomp audit trace record")?;
        out.write_all(b"\n")
            .context("failed to write seccomp audit trace record")?;
    }
    out.flush()
        .context("failed to flush seccomp audit trace records")?;
    out.sync_all()
        .context("failed to sync seccomp audit trace records")?;
    Ok(())
}

fn finalized_trace_temp_path(trace_path: &Path) -> PathBuf {
    let mut name = trace_path
        .file_name()
        .map(|name| name.to_os_string())
        .unwrap_or_else(|| "loftd-seccomp-trace".into());
    name.push(format!(".tmp-{}", std::process::id()));
    trace_path.with_file_name(name)
}

pub(crate) fn synthesize_policy(input: &Path, output: &Path) -> Result<()> {
    let syscalls = syscalls_from_trace(input)?;
    if syscalls.is_empty() {
        bail!(
            "seccomp trace '{}' did not contain any syscalls",
            input.display()
        );
    }
    let policy = SeccompilerPolicy {
        main_thread: ThreadPolicy {
            mismatch_action: "trap",
            match_action: "allow",
            filter: syscalls
                .into_iter()
                .map(|syscall| SyscallRule { syscall })
                .collect(),
        },
    };
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!("failed to create seccomp policy dir '{}'", parent.display())
        })?;
    }
    let mut file = fs::File::create(output)
        .with_context(|| format!("failed to create seccomp policy '{}'", output.display()))?;
    serde_json::to_writer_pretty(&mut file, &policy).context("failed to write seccomp policy")?;
    file.write_all(b"\n")
        .context("failed to finish seccomp policy")?;
    Ok(())
}

pub(crate) fn strace_exclusion_filter_from_policy(policy_path: &Path) -> Result<String> {
    let syscalls = allowed_syscalls_from_policy(policy_path)?;
    if syscalls.is_empty() {
        bail!(
            "seccomp gap audit baseline policy '{}' has an empty main_thread.filter allowlist",
            policy_path.display()
        );
    }
    Ok(format!(
        "trace=!{}",
        syscalls.into_iter().collect::<Vec<_>>().join(",")
    ))
}

pub(crate) fn allowed_syscalls_from_policy(policy_path: &Path) -> Result<BTreeSet<String>> {
    let text = fs::read_to_string(policy_path).with_context(|| {
        format!(
            "failed to read seccomp baseline policy '{}'",
            policy_path.display()
        )
    })?;
    let policy = serde_json::from_str::<serde_json::Value>(&text).with_context(|| {
        format!(
            "failed to parse seccomp baseline policy '{}'",
            policy_path.display()
        )
    })?;
    allowed_syscalls_from_policy_value(&policy).with_context(|| {
        format!(
            "failed to inspect seccomp baseline policy '{}'",
            policy_path.display()
        )
    })
}

fn allowed_syscalls_from_policy_value(policy: &serde_json::Value) -> Result<BTreeSet<String>> {
    let filter = policy
        .get("main_thread")
        .and_then(|value| value.get("filter"))
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| anyhow!("seccomp policy main_thread.filter must be an array"))?;
    let mut syscalls = BTreeSet::new();
    for (index, rule) in filter.iter().enumerate() {
        let syscall = rule
            .get("syscall")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| {
                anyhow!("seccomp policy main_thread.filter[{index}].syscall must be a string")
            })?;
        if !valid_syscall_name(syscall) {
            bail!(
                "invalid syscall name '{}' in seccomp policy main_thread.filter[{index}]",
                syscall
            );
        }
        syscalls.insert(syscall.to_owned());
    }
    Ok(syscalls)
}

pub(crate) fn extend_policy(policy: &Path, trace: &Path, output: &Path) -> Result<()> {
    if policy == output {
        bail!(
            "refusing to overwrite baseline seccomp policy '{}'",
            policy.display()
        );
    }
    let text = fs::read_to_string(policy).with_context(|| {
        format!(
            "failed to read baseline seccomp policy '{}'",
            policy.display()
        )
    })?;
    let mut policy_value = serde_json::from_str::<serde_json::Value>(&text).with_context(|| {
        format!(
            "failed to parse baseline seccomp policy '{}'",
            policy.display()
        )
    })?;
    let existing = allowed_syscalls_from_policy_value(&policy_value).with_context(|| {
        format!(
            "failed to inspect baseline seccomp policy '{}'",
            policy.display()
        )
    })?;
    let observed = syscalls_from_trace(trace)?;
    let additions = observed
        .difference(&existing)
        .cloned()
        .collect::<Vec<String>>();

    let filter = policy_value
        .get_mut("main_thread")
        .and_then(|value| value.get_mut("filter"))
        .and_then(serde_json::Value::as_array_mut)
        .ok_or_else(|| anyhow!("seccomp policy main_thread.filter must be an array"))?;
    for syscall in additions {
        filter.push(serde_json::json!({ "syscall": syscall }));
    }

    let mut bytes = Vec::new();
    serde_json::to_writer_pretty(&mut bytes, &policy_value)
        .context("failed to encode extended seccomp policy")?;
    bytes.push(b'\n');
    validate_seccomp_policy_bytes(&bytes, output)?;

    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!("failed to create seccomp policy dir '{}'", parent.display())
        })?;
    }
    let mut file = fs::File::create(output).with_context(|| {
        format!(
            "failed to create extended seccomp policy '{}'",
            output.display()
        )
    })?;
    file.write_all(&bytes)
        .context("failed to write extended seccomp policy")?;
    Ok(())
}

fn validate_seccomp_policy_bytes(policy_bytes: &[u8], output: &Path) -> Result<()> {
    let arch = std::env::consts::ARCH.try_into().map_err(|_| {
        anyhow!(
            "seccomp does not support host architecture {}",
            std::env::consts::ARCH
        )
    })?;
    seccompiler::compile_from_json(Cursor::new(policy_bytes), arch).with_context(|| {
        format!(
            "extended seccomp policy '{}' is not valid for seccompiler",
            output.display()
        )
    })?;
    Ok(())
}

pub(crate) fn apply_enforce_policy(policy_path: &Path) -> Result<()> {
    let policy = fs::File::open(policy_path)
        .with_context(|| format!("failed to open seccomp policy '{}'", policy_path.display()))?;
    let arch = std::env::consts::ARCH.try_into().map_err(|_| {
        anyhow!(
            "seccomp does not support host architecture {}",
            std::env::consts::ARCH
        )
    })?;
    let filters = seccompiler::compile_from_json(policy, arch).with_context(|| {
        format!(
            "failed to compile seccomp policy '{}'",
            policy_path.display()
        )
    })?;
    let filter = filters
        .get("main_thread")
        .ok_or_else(|| anyhow!("seccomp policy must contain a main_thread filter"))?;
    set_no_new_privs()?;
    seccompiler::apply_filter(filter).with_context(|| {
        format!(
            "failed to install seccomp policy '{}'",
            policy_path.display()
        )
    })
}

fn set_no_new_privs() -> Result<()> {
    // SAFETY: prctl is called with PR_SET_NO_NEW_PRIVS and constant integer
    // arguments before loading the seccomp filter in the current VM worker.
    let rc = unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) };
    if rc == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error()).context("failed to set PR_SET_NO_NEW_PRIVS")
    }
}

fn syscalls_from_trace(input: &Path) -> Result<BTreeSet<String>> {
    let file = fs::File::open(input)
        .with_context(|| format!("failed to open seccomp trace '{}'", input.display()))?;
    let mut syscalls = BTreeSet::new();
    for line in BufReader::new(file).lines() {
        let line = line.with_context(|| format!("failed to read '{}'", input.display()))?;
        if line.trim().is_empty() {
            continue;
        }
        if line.trim_start().starts_with('{') {
            let record = serde_json::from_str::<TraceRecord>(&line).with_context(|| {
                format!(
                    "invalid seccomp JSONL trace record in '{}'",
                    input.display()
                )
            })?;
            if !valid_syscall_name(&record.syscall) {
                bail!(
                    "invalid syscall name '{}' in seccomp trace '{}'",
                    record.syscall,
                    input.display()
                );
            }
            syscalls.insert(record.syscall);
        } else if let Some(syscall) = syscall_from_strace_line(&line) {
            syscalls.insert(syscall);
        }
    }
    Ok(syscalls)
}

fn syscall_from_strace_line(line: &str) -> Option<String> {
    let line = strip_pid_prefix(line.trim());
    if line.is_empty() || line.starts_with("+++") || line.starts_with("---") {
        return None;
    }
    if let Some(rest) = line.strip_prefix("<... ") {
        let syscall = rest.split_whitespace().next()?.trim();
        return valid_syscall_name(syscall).then(|| syscall.to_owned());
    }
    let open_paren = line.find('(')?;
    let syscall = line[..open_paren].trim();
    valid_syscall_name(syscall).then(|| syscall.to_owned())
}

fn strip_pid_prefix(line: &str) -> &str {
    if let Some(rest) = line.strip_prefix("[pid ") {
        let Some((_, after)) = rest.split_once(']') else {
            return line;
        };
        return after.trim_start();
    }

    let Some((pid, after)) = line.split_once(char::is_whitespace) else {
        return line;
    };
    if !pid.is_empty() && pid.bytes().all(|byte| byte.is_ascii_digit()) {
        after.trim_start()
    } else {
        line
    }
}

fn valid_syscall_name(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TraceRecord {
    syscall: String,
    raw: String,
}

#[derive(Debug, Clone, Serialize)]
struct SeccompilerPolicy {
    main_thread: ThreadPolicy,
}

#[derive(Debug, Clone, Serialize)]
struct ThreadPolicy {
    mismatch_action: &'static str,
    match_action: &'static str,
    filter: Vec<SyscallRule>,
}

#[derive(Debug, Clone, Serialize)]
struct SyscallRule {
    syscall: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_common_strace_lines() {
        assert_eq!(
            syscall_from_strace_line("[pid 123] openat(AT_FDCWD, \"/tmp\", O_RDONLY) = 3"),
            Some("openat".to_owned())
        );
        assert_eq!(
            syscall_from_strace_line(
                "40860 execve(\"/nix/store/bin/loftd\", [\"loftd\"], 0x1234) = 0"
            ),
            Some("execve".to_owned())
        );
        assert_eq!(
            syscall_from_strace_line("<... read resumed> \"\", 8192) = 0"),
            Some("read".to_owned())
        );
        assert_eq!(syscall_from_strace_line("+++ exited with 0 +++"), None);
    }

    #[test]
    fn prepares_audit_trace_and_raw_trace_parent_dirs() {
        let dir = tempfile::tempdir().expect("tempdir");
        let trace = dir.path().join("nested").join("trace.jsonl");

        prepare_audit_trace_target(&trace).expect("prepare trace target");

        assert!(trace.parent().expect("trace parent").is_dir());
        assert!(
            raw_strace_path(&trace)
                .parent()
                .expect("raw parent")
                .is_dir()
        );
    }

    #[test]
    fn finalizes_raw_strace_to_jsonl_atomically() {
        let dir = tempfile::tempdir().expect("tempdir");
        let trace = dir.path().join("trace.jsonl");
        let raw = raw_strace_path(&trace);
        fs::write(
            &raw,
            "123 read(0, \"\", 1) = 0\n123 write(1, \"x\", 1) = 1\n",
        )
        .expect("write raw trace");

        finalize_audit_trace(&trace).expect("finalize trace");

        let syscalls = syscalls_from_trace(&trace).expect("read finalized trace");
        assert_eq!(
            syscalls,
            BTreeSet::from(["read".to_owned(), "write".to_owned()])
        );
        assert!(!finalized_trace_temp_path(&trace).exists());
    }

    #[test]
    fn finalization_failure_preserves_existing_trace() {
        let dir = tempfile::tempdir().expect("tempdir");
        let trace = dir.path().join("trace.jsonl");
        let raw = raw_strace_path(&trace);
        fs::write(&trace, "old finalized trace\n").expect("write old trace");
        fs::write(&raw, b"123 read(0, \"\", 1) = 0\n\xff\n").expect("write bad raw trace");

        let err = finalize_audit_trace(&trace).expect_err("bad raw trace should fail");

        assert!(format!("{err:#}").contains("failed to read"));
        assert_eq!(
            fs::read_to_string(&trace).expect("old trace should remain"),
            "old finalized trace\n"
        );
        assert!(!finalized_trace_temp_path(&trace).exists());
    }

    #[test]
    fn synthesizes_deterministic_seccompiler_policy() {
        let dir = tempfile::tempdir().expect("tempdir");
        let input = dir.path().join("trace.jsonl");
        let output = dir.path().join("policy.json");
        fs::write(
            &input,
            "{\"syscall\":\"write\",\"raw\":\"write(1, \\\"x\\\", 1) = 1\"}\nread(0, \"\",\n",
        )
        .expect("write trace");

        synthesize_policy(&input, &output).expect("synthesize");

        let policy = fs::read_to_string(&output).expect("read policy");
        assert!(policy.contains("\"mismatch_action\": \"trap\""));
        assert!(policy.find("\"read\"").unwrap() < policy.find("\"write\"").unwrap());
        let file = fs::File::open(output).expect("open policy");
        let arch = std::env::consts::ARCH.try_into().expect("supported arch");
        let filters = seccompiler::compile_from_json(file, arch).expect("policy should compile");
        assert!(filters.contains_key("main_thread"));
    }

    #[test]
    fn synthesize_rejects_malformed_jsonl_records() {
        let dir = tempfile::tempdir().expect("tempdir");
        let input = dir.path().join("trace.jsonl");
        let output = dir.path().join("policy.json");
        fs::write(&input, "{\"syscall\":\"read\",\"raw\":\"unterminated}\n").expect("write trace");

        let err = synthesize_policy(&input, &output).expect_err("malformed JSONL should fail");

        assert!(format!("{err:#}").contains("invalid seccomp JSONL trace record"));
    }

    #[test]
    fn extracts_allowed_syscalls_from_baseline_policy() {
        let policy = serde_json::json!({
            "main_thread": {
                "mismatch_action": "trap",
                "match_action": "allow",
                "filter": [
                    { "syscall": "write" },
                    { "syscall": "read" }
                ]
            }
        });

        let syscalls =
            allowed_syscalls_from_policy_value(&policy).expect("allowed syscalls should parse");

        assert_eq!(
            syscalls,
            BTreeSet::from(["read".to_owned(), "write".to_owned()])
        );
    }

    #[test]
    fn rejects_invalid_baseline_policy_filter_entries() {
        let missing_filter = serde_json::json!({
            "main_thread": {
                "mismatch_action": "trap",
                "match_action": "allow"
            }
        });
        let bad_name = serde_json::json!({
            "main_thread": {
                "mismatch_action": "trap",
                "match_action": "allow",
                "filter": [
                    { "syscall": "BadName" }
                ]
            }
        });

        let missing_err = allowed_syscalls_from_policy_value(&missing_filter)
            .expect_err("missing filter should fail");
        let bad_name_err =
            allowed_syscalls_from_policy_value(&bad_name).expect_err("bad syscall should fail");

        assert!(format!("{missing_err:#}").contains("main_thread.filter must be an array"));
        assert!(format!("{bad_name_err:#}").contains("invalid syscall name"));
    }

    #[test]
    fn gap_audit_rejects_empty_baseline_allowlist() {
        let dir = tempfile::tempdir().expect("tempdir");
        let policy = dir.path().join("empty-policy.json");
        fs::write(
            &policy,
            r#"{
              "main_thread": {
                "mismatch_action": "trap",
                "match_action": "allow",
                "filter": []
              }
            }"#,
        )
        .expect("write policy");

        let err =
            strace_exclusion_filter_from_policy(&policy).expect_err("empty allowlist should fail");

        assert!(format!("{err:#}").contains("empty main_thread.filter allowlist"));
    }

    #[test]
    fn builds_deterministic_gap_audit_strace_filter() {
        let dir = tempfile::tempdir().expect("tempdir");
        let policy = dir.path().join("policy.json");
        fs::write(
            &policy,
            r#"{
              "main_thread": {
                "mismatch_action": "trap",
                "match_action": "allow",
                "filter": [
                  { "syscall": "write" },
                  { "syscall": "read" }
                ]
              }
            }"#,
        )
        .expect("write policy");

        let filter = strace_exclusion_filter_from_policy(&policy).expect("gap filter");

        assert_eq!(filter, "trace=!read,write");
    }

    #[test]
    fn extend_policy_adds_missing_syscalls_without_mutating_baseline() {
        let dir = tempfile::tempdir().expect("tempdir");
        let policy = dir.path().join("policy.json");
        let trace = dir.path().join("denied.jsonl");
        let output = dir.path().join("updated.json");
        fs::write(
            &policy,
            r#"{
              "main_thread": {
                "mismatch_action": "trap",
                "match_action": "allow",
                "filter": [
                  { "syscall": "read" }
                ]
              }
            }"#,
        )
        .expect("write policy");
        let original = fs::read_to_string(&policy).expect("read original policy");
        fs::write(
            &trace,
            "{\"syscall\":\"write\",\"raw\":\"write(1, \\\"x\\\", 1) = 1\"}\n{\"syscall\":\"openat\",\"raw\":\"openat(AT_FDCWD, \\\"/x\\\", O_RDONLY) = 3\"}\n{\"syscall\":\"read\",\"raw\":\"read(0, \\\"\\\", 1) = 0\"}\n",
        )
        .expect("write trace");

        extend_policy(&policy, &trace, &output).expect("extend policy");

        assert_eq!(
            fs::read_to_string(&policy).expect("baseline still readable"),
            original
        );
        let updated = fs::read_to_string(&output).expect("read updated policy");
        assert!(updated.find("\"read\"").unwrap() < updated.find("\"openat\"").unwrap());
        assert!(updated.find("\"openat\"").unwrap() < updated.find("\"write\"").unwrap());
        let file = fs::File::open(output).expect("open updated policy");
        let arch = std::env::consts::ARCH.try_into().expect("supported arch");
        let filters = seccompiler::compile_from_json(file, arch).expect("policy should compile");
        assert!(filters.contains_key("main_thread"));
    }

    #[test]
    fn extend_policy_refuses_to_overwrite_baseline() {
        let dir = tempfile::tempdir().expect("tempdir");
        let policy = dir.path().join("policy.json");
        let trace = dir.path().join("trace.jsonl");
        fs::write(&policy, "{}").expect("write policy");
        fs::write(&trace, "").expect("write trace");

        let err = extend_policy(&policy, &trace, &policy).expect_err("same output should fail");

        assert!(format!("{err:#}").contains("refusing to overwrite baseline"));
    }
}
