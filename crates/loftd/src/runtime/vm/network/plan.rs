//! pasta/passt command-plan construction.

use anyhow::Result;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::runtime::publish::{PasstPublishProtocol, passt_publish_specs};

use super::addresses::HOST_GATEWAY_ADDR;

const HOST_DNS_FORWARD_ADDR: &str = "169.254.1.1";
pub(crate) const PASTA_PROGRAM: &str = "pasta";
pub(crate) const PASST_PROGRAM: &str = "passt";
pub(crate) const PASST_SOCKET_FILE: &str = "passt.sock";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProxyCommandPlan {
    pub(crate) program: String,
    pub(crate) args: Vec<String>,
    pub(crate) socket: Option<PathBuf>,
}

impl ProxyCommandPlan {
    pub(super) fn command(&self) -> Command {
        let mut command = Command::new(&self.program);
        command
            .args(&self.args)
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());
        command
    }
}

pub(crate) fn pasta_plan(holder_pid: libc::pid_t) -> ProxyCommandPlan {
    ProxyCommandPlan {
        program: PASTA_PROGRAM.to_owned(),
        args: vec![
            "--foreground".to_owned(),
            "--config-net".to_owned(),
            "--no-map-gw".to_owned(),
            "--map-guest-addr".to_owned(),
            HOST_GATEWAY_ADDR.to_owned(),
            "--dns-forward".to_owned(),
            HOST_DNS_FORWARD_ADDR.to_owned(),
            "-t".to_owned(),
            "none".to_owned(),
            "-u".to_owned(),
            "none".to_owned(),
            "-T".to_owned(),
            "none".to_owned(),
            "-U".to_owned(),
            "none".to_owned(),
            "--quiet".to_owned(),
            "--netns".to_owned(),
            format!("/proc/{holder_pid}/ns/net"),
        ],
        socket: None,
    }
}

pub(crate) fn passt_plan(task_state_dir: &Path, publish: &[String]) -> Result<ProxyCommandPlan> {
    let socket = proxy_artifact_path(task_state_dir, PASST_SOCKET_FILE);
    let mut args = vec![
        "--foreground".to_owned(),
        "--one-off".to_owned(),
        "--socket".to_owned(),
        socket.display().to_string(),
        "--map-guest-addr".to_owned(),
        HOST_GATEWAY_ADDR.to_owned(),
        "--dns-forward".to_owned(),
        HOST_DNS_FORWARD_ADDR.to_owned(),
    ];
    append_passt_publish_args(&mut args, publish)?;
    args.push("--quiet".to_owned());

    Ok(ProxyCommandPlan {
        program: PASST_PROGRAM.to_owned(),
        args,
        socket: Some(socket),
    })
}

fn append_passt_publish_args(args: &mut Vec<String>, publish: &[String]) -> Result<()> {
    let specs = passt_publish_specs(publish)?;
    let mut has_tcp = false;
    let mut has_udp = false;

    for spec in specs {
        match spec.protocol {
            PasstPublishProtocol::Tcp => {
                args.push("-t".to_owned());
                args.push(spec.payload);
                has_tcp = true;
            }
            PasstPublishProtocol::Udp => {
                args.push("-u".to_owned());
                args.push(spec.payload);
                has_udp = true;
            }
        }
    }

    if !has_tcp {
        args.push("-t".to_owned());
        args.push("none".to_owned());
    }
    if !has_udp {
        args.push("-u".to_owned());
        args.push("none".to_owned());
    }

    Ok(())
}

fn proxy_artifact_path(task_state_dir: &Path, file_name: &str) -> PathBuf {
    let task_id = task_state_dir
        .file_name()
        .and_then(|value| value.to_str())
        .map(sanitize_proxy_artifact_component)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "task".to_owned());
    PathBuf::from("/tmp").join(format!(
        "loftd-{}-{}-{file_name}",
        std::process::id(),
        task_id
    ))
}

fn sanitize_proxy_artifact_component(value: &str) -> String {
    value
        .chars()
        .map(|ch| match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' => ch,
            _ => '-',
        })
        .collect()
}
