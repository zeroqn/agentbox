use anyhow::{Context, Result};

use crate::podman::command::run_podman_output;

pub(in crate::container::nix_sidecar) const SIDECAR_PROXY_CONTAINER_PORT: &str = "19876";
pub(in crate::container::nix_sidecar) const SIDECAR_PROXY_FALLBACK_PORT: u16 = 19876;

pub(in crate::container::nix_sidecar) fn resolve_sidecar_proxy_port(
    sidecar_name: &str,
) -> Result<u16> {
    let output = run_podman_output(
        vec![
            "port".to_owned(),
            sidecar_name.to_owned(),
            SIDECAR_PROXY_CONTAINER_PORT.to_owned(),
        ],
        "failed to resolve sidecar proxy port",
    )?;
    let port_str = output
        .trim()
        .lines()
        .next()
        .and_then(|line| line.rsplit(':').next())
        .with_context(|| {
            format!(
                "unexpected 'podman port' output for '{}': {:?}",
                sidecar_name, output
            )
        })?;
    port_str
        .parse::<u16>()
        .with_context(|| format!("invalid proxy port number: {port_str}"))
}

pub(in crate::container::nix_sidecar) fn resolve_runtime_proxy_port_or_default(
    result: Result<u16>,
) -> u16 {
    result.unwrap_or_else(|err| {
        eprintln!("agentbox: warning: failed to resolve sidecar proxy port: {err:#}");
        SIDECAR_PROXY_FALLBACK_PORT
    })
}
