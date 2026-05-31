use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TaskRootfsBackend {
    BtrfsSnapshot,
    FuseOverlay,
}

impl TaskRootfsBackend {
    pub(crate) const DEFAULT: Self = Self::BtrfsSnapshot;

    pub(crate) fn parse_config_value(value: &str) -> Result<Self, String> {
        match value {
            "btrfs-snapshot" => Ok(Self::BtrfsSnapshot),
            "fuse-overlay" => Ok(Self::FuseOverlay),
            _ => Err(format!(
                "task rootfs backend must be one of: btrfs-snapshot, fuse-overlay (got '{value}')"
            )),
        }
    }

    pub(crate) fn as_config_value(self) -> &'static str {
        match self {
            Self::BtrfsSnapshot => "btrfs-snapshot",
            Self::FuseOverlay => "fuse-overlay",
        }
    }
}

impl fmt::Display for TaskRootfsBackend {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_config_value())
    }
}

#[cfg(test)]
mod tests {
    use crate::task_rootfs::TaskRootfsBackend;

    #[test]
    fn parses_allowed_backend_values() {
        assert_eq!(
            TaskRootfsBackend::parse_config_value("btrfs-snapshot")
                .expect("btrfs backend should parse"),
            TaskRootfsBackend::BtrfsSnapshot
        );
        assert_eq!(
            TaskRootfsBackend::parse_config_value("fuse-overlay")
                .expect("fuse backend should parse"),
            TaskRootfsBackend::FuseOverlay
        );
    }

    #[test]
    fn rejects_auto_and_reflink_backends() {
        let auto_err = TaskRootfsBackend::parse_config_value("auto")
            .expect_err("auto backend should be rejected");
        let reflink_err = TaskRootfsBackend::parse_config_value("reflink")
            .expect_err("reflink backend should be rejected");

        assert!(auto_err.contains("btrfs-snapshot"));
        assert!(reflink_err.contains("fuse-overlay"));
    }
}
