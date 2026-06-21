//! Host-side seccomp audit, synthesis, and enforcement support.

use anyhow::{Context, Result, anyhow, bail};
use clap::Subcommand;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) enum SeccompMode {
    #[default]
    Off,
    Audit {
        trace_path: PathBuf,
    },
    Enforce {
        policy_path: PathBuf,
    },
}

impl SeccompMode {
    pub(crate) fn as_config_value(&self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Audit { .. } => "audit",
            Self::Enforce { .. } => "enforce",
        }
    }

    pub(crate) fn parse_config_value(
        mode: Option<&str>,
        audit_trace_path: Option<&str>,
        enforce_policy_path: Option<&str>,
    ) -> Result<Self> {
        match mode.unwrap_or("off") {
            "off" => Ok(Self::Off),
            "audit" => Ok(Self::Audit {
                trace_path: PathBuf::from(audit_trace_path.ok_or_else(|| {
                    anyhow!("loftd launch config seccomp.audit_trace_path is required")
                })?),
            }),
            "enforce" => Ok(Self::Enforce {
                policy_path: PathBuf::from(enforce_policy_path.ok_or_else(|| {
                    anyhow!("loftd launch config seccomp.enforce_policy_path is required")
                })?),
            }),
            _ => bail!("loftd launch config seccomp.mode is invalid"),
        }
    }

    pub(crate) fn audit_trace_path(&self) -> Option<&Path> {
        match self {
            Self::Audit { trace_path } => Some(trace_path),
            Self::Off | Self::Enforce { .. } => None,
        }
    }

    pub(crate) fn enforce_policy_path(&self) -> Option<&Path> {
        match self {
            Self::Enforce { policy_path } => Some(policy_path),
            Self::Off | Self::Audit { .. } => None,
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
    "seccomp audit mode uses strace/ptrace; if ptrace is disabled on NixOS, check `boot.kernel.sysctl.\"kernel.yama.ptrace_scope\"` and temporarily set `sudo sysctl kernel.yama.ptrace_scope=0` for the audit run"
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
    let raw = fs::File::open(&raw_path)
        .with_context(|| format!("failed to open raw strace log '{}'", raw_path.display()))?;
    if let Some(parent) = trace_path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!("failed to create seccomp trace dir '{}'", parent.display())
        })?;
    }
    let mut out = fs::File::create(trace_path)
        .with_context(|| format!("failed to create seccomp trace '{}'", trace_path.display()))?;
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
    Ok(())
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
    let Some(rest) = line.strip_prefix("[pid ") else {
        return line;
    };
    let Some((_, after)) = rest.split_once(']') else {
        return line;
    };
    after.trim_start()
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
}
