pub(in crate::runtime::session) const PTY_RAW_PASSTHROUGH_ENV: &str = "LOFTD_PTY_RAW_PASSTHROUGH";

pub(in crate::runtime::session) fn guest_env_pair(flag_enabled: bool) -> Option<(String, String)> {
    guest_env_pair_from_value(
        flag_enabled,
        std::env::var(PTY_RAW_PASSTHROUGH_ENV).ok().as_deref(),
    )
}

pub(in crate::runtime::session) fn guest_env_pair_from_value(
    flag_enabled: bool,
    value: Option<&str>,
) -> Option<(String, String)> {
    (flag_enabled || value.is_some_and(env_value_enabled))
        .then(|| (PTY_RAW_PASSTHROUGH_ENV.to_owned(), "1".to_owned()))
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
    fn pty_raw_passthrough_env_pair_accepts_cli_flag_without_env() {
        assert_eq!(
            guest_env_pair_from_value(true, None),
            Some((PTY_RAW_PASSTHROUGH_ENV.to_owned(), "1".to_owned()))
        );
    }

    #[test]
    fn pty_raw_passthrough_env_pair_accepts_cli_flag_with_falsey_env() {
        assert_eq!(
            guest_env_pair_from_value(true, Some("0")),
            Some((PTY_RAW_PASSTHROUGH_ENV.to_owned(), "1".to_owned()))
        );
        assert_eq!(
            guest_env_pair_from_value(true, Some("false")),
            Some((PTY_RAW_PASSTHROUGH_ENV.to_owned(), "1".to_owned()))
        );
    }

    #[test]
    fn pty_raw_passthrough_env_pair_accepts_truthy_env_without_cli_flag() {
        assert_eq!(
            guest_env_pair_from_value(false, Some("YES")),
            Some((PTY_RAW_PASSTHROUGH_ENV.to_owned(), "1".to_owned()))
        );
    }

    #[test]
    fn pty_raw_passthrough_env_pair_rejects_missing_falsey_or_invalid_env_without_cli_flag() {
        assert_eq!(guest_env_pair_from_value(false, None), None);
        assert_eq!(guest_env_pair_from_value(false, Some("")), None);
        assert_eq!(guest_env_pair_from_value(false, Some("0")), None);
        assert_eq!(guest_env_pair_from_value(false, Some("false")), None);
        assert_eq!(guest_env_pair_from_value(false, Some("summary")), None);
    }
}
