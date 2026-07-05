pub(in crate::runtime::session) const PTY_RAW_PASSTHROUGH_ENV: &str = "LOFTD_PTY_RAW_PASSTHROUGH";

pub(in crate::runtime::session) fn guest_env_pair_from_process_env() -> Option<(String, String)> {
    guest_env_pair_from_value(std::env::var(PTY_RAW_PASSTHROUGH_ENV).ok().as_deref())
}

pub(in crate::runtime::session) fn guest_env_pair_from_value(
    value: Option<&str>,
) -> Option<(String, String)> {
    value
        .filter(|value| env_value_enabled(value))
        .map(|_| (PTY_RAW_PASSTHROUGH_ENV.to_owned(), "1".to_owned()))
}

fn env_value_enabled(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pty_raw_passthrough_env_pair_requires_truthy_value() {
        assert_eq!(guest_env_pair_from_value(None), None);
        assert_eq!(guest_env_pair_from_value(Some("")), None);
        assert_eq!(guest_env_pair_from_value(Some("0")), None);
        assert_eq!(guest_env_pair_from_value(Some("false")), None);
        assert_eq!(guest_env_pair_from_value(Some("summary")), None);
        assert_eq!(
            guest_env_pair_from_value(Some("YES")),
            Some((PTY_RAW_PASSTHROUGH_ENV.to_owned(), "1".to_owned()))
        );
    }
}
