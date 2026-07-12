use anyhow::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GpuMode {
    Off,
    Drm,
}

impl GpuMode {
    pub(crate) fn as_config_value(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Drm => "drm",
        }
    }

    pub(crate) fn parse_config_value(value: &str) -> Result<Self> {
        match value {
            "off" => Ok(Self::Off),
            "drm" => Ok(Self::Drm),
            _ => anyhow::bail!("loftd launch config gpu mode is invalid"),
        }
    }
}
