//! pasta/passt command-plan construction.

use anyhow::Result;
use std::process::{Command, Stdio};

use crate::runtime::publish::{PasstPublishProtocol, passt_publish_specs};

use super::addresses::HOST_GATEWAY_ADDR;

const HOST_DNS_FORWARD_ADDR: &str = "169.254.1.1";
pub(crate) const PASTA_PROGRAM: &str = "pasta";
pub(crate) const PASST_PROGRAM: &str = "passt";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProxyCommandPlan {
    pub(crate) program: String,
    pub(crate) args: Vec<String>,
    pub(crate) fd: Option<i32>,
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

pub(crate) fn pasta_plan(holder_pid: libc::pid_t, tcp_forwards: &[String]) -> ProxyCommandPlan {
    let mut args = vec![
        "--foreground".to_owned(),
        "--config-net".to_owned(),
        "--no-map-gw".to_owned(),
        "--map-guest-addr".to_owned(),
        HOST_GATEWAY_ADDR.to_owned(),
        "--dns-forward".to_owned(),
        HOST_DNS_FORWARD_ADDR.to_owned(),
    ];
    if tcp_forwards.is_empty() {
        args.push("-t".to_owned());
        args.push("none".to_owned());
    } else {
        for forward in tcp_forwards {
            args.push("-t".to_owned());
            args.push(forward.clone());
        }
    }
    args.extend([
        "-u".to_owned(),
        "none".to_owned(),
        "-T".to_owned(),
        "none".to_owned(),
        "-U".to_owned(),
        "none".to_owned(),
        "--quiet".to_owned(),
        "--netns".to_owned(),
        format!("/proc/{holder_pid}/ns/net"),
    ]);

    ProxyCommandPlan {
        program: PASTA_PROGRAM.to_owned(),
        args,
        fd: None,
    }
}

pub(crate) fn passt_plan(fd: i32, publish: &[String]) -> Result<ProxyCommandPlan> {
    let mut args = vec![
        "--foreground".to_owned(),
        "--fd".to_owned(),
        fd.to_string(),
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
        fd: Some(fd),
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
