pub(in crate::runtime::session) const PTY_RAW_PASSTHROUGH_ENV: &str = "LOFTD_PTY_RAW_PASSTHROUGH";

pub(in crate::runtime::session) fn guest_env_pair(
    mode: crate::cli::PtyMode,
) -> Option<(String, String)> {
    (mode == crate::cli::PtyMode::RawPassthrough)
        .then(|| (PTY_RAW_PASSTHROUGH_ENV.to_owned(), "1".to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::PtyMode;

    #[test]
    fn pty_raw_passthrough_env_pair_accepts_raw_cli_mode() {
        assert_eq!(
            guest_env_pair(PtyMode::RawPassthrough),
            Some((PTY_RAW_PASSTHROUGH_ENV.to_owned(), "1".to_owned()))
        );
    }

    #[test]
    fn host_raw_passthrough_env_is_not_public_fallback() {
        let old = std::env::var_os(PTY_RAW_PASSTHROUGH_ENV);
        unsafe { std::env::set_var(PTY_RAW_PASSTHROUGH_ENV, "1") };

        let pair = guest_env_pair(PtyMode::Normalized);

        unsafe {
            if let Some(old) = old {
                std::env::set_var(PTY_RAW_PASSTHROUGH_ENV, old);
            } else {
                std::env::remove_var(PTY_RAW_PASSTHROUGH_ENV);
            }
        }
        assert_eq!(pair, None);
    }

    #[test]
    fn pty_raw_passthrough_env_pair_rejects_normalized_cli_mode() {
        assert_eq!(guest_env_pair(PtyMode::Normalized), None);
    }
}
