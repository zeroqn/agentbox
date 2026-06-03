use anyhow::{Context, Result, bail};
use std::path::Path;

use crate::guest_init::fs;

pub(in crate::guest_init) fn prepare() -> Result<()> {
    enable_user_namespaces()?;
    prepare_tun_device()?;
    prepare_kvm_device()
}

fn enable_user_namespaces() -> Result<()> {
    maybe_raise_sysctl(Path::new("/proc/sys/user/max_user_namespaces"), 28_633)?;
    if Path::new("/proc/sys/kernel/unprivileged_userns_clone").exists() {
        maybe_raise_sysctl(Path::new("/proc/sys/kernel/unprivileged_userns_clone"), 1)?;
    }
    Ok(())
}

fn maybe_raise_sysctl(path: &Path, target: u32) -> Result<()> {
    if !path.exists() {
        bail!(
            "kernel does not expose {}; rootless container runtimes need user namespace support",
            path.display()
        );
    }
    let current = std::fs::read_to_string(path)
        .ok()
        .and_then(|text| text.trim().parse::<u32>().ok())
        .unwrap_or(0);
    if current < target {
        std::fs::write(path, format!("{target}\n")).with_context(|| {
            format!(
                "failed to set {}={target} for rootless container runtimes",
                path.display()
            )
        })?;
    }
    Ok(())
}

fn prepare_tun_device() -> Result<()> {
    let tun = Path::new("/dev/net/tun");
    if !tun.exists() {
        bail!(
            "rootless container runtime TUN device is missing at {}; ensure host /dev/net/tun is passed into the internal guest",
            tun.display()
        );
    }
    fs::chmod(tun, 0o666)
        .context("failed to make /dev/net/tun accessible to rootless container runtimes")
}

fn prepare_kvm_device() -> Result<()> {
    prepare_kvm_device_at(Path::new("/dev/kvm"))
}

fn prepare_kvm_device_at(kvm: &Path) -> Result<()> {
    if !kvm.exists() {
        return Ok(());
    }
    fs::chmod(kvm, 0o666).with_context(|| {
        format!(
            "failed to make {} accessible to non-root nested KVM tasks",
            kvm.display()
        )
    })
}

#[cfg(test)]
#[path = "kernel_tests.rs"]
mod tests;
