use anyhow::Result;

pub(crate) fn prepare() -> Result<()> {
    anyhow::bail!(
        "libkrun mode is not available until raw_image Nix support is implemented; container mode remains the default; no sidecar/overlay/seeded fallback is used"
    );
}
