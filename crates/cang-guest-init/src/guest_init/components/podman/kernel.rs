use anyhow::Result;

pub(in crate::guest_init) fn prepare() -> Result<()> {
    crate::guest_init::components::rootless::kernel::prepare()
}
