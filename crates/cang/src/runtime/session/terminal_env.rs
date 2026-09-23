//! Host terminal identity propagation for managed guest PTY sessions.

use std::env;
use std::ffi::OsString;

const HOST_TERMINAL_ENV_NAMES: [&str; 4] =
    ["TERM", "COLORTERM", "TERM_PROGRAM", "TERM_PROGRAM_VERSION"];

pub(crate) fn host_terminal_env_pairs() -> Vec<(String, String)> {
    host_terminal_env_pairs_from(|name| env::var_os(name))
}

fn host_terminal_env_pairs_from(
    mut lookup: impl FnMut(&str) -> Option<OsString>,
) -> Vec<(String, String)> {
    HOST_TERMINAL_ENV_NAMES
        .iter()
        .filter_map(|name| {
            let value = lookup(name)?.into_string().ok()?;
            if value.is_empty() {
                return None;
            }
            Some(((*name).to_owned(), value))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lookup_from<'a>(
        entries: &'a [(&'a str, OsString)],
    ) -> impl FnMut(&str) -> Option<OsString> + 'a {
        move |name| {
            entries
                .iter()
                .find_map(|(key, value)| (*key == name).then(|| value.clone()))
        }
    }

    #[test]
    fn terminal_env_collects_exact_non_empty_canonical_values() {
        let entries = [
            ("TERM", OsString::from("xterm-kitty")),
            ("COLORTERM", OsString::from("truecolor")),
            ("TERM_PROGRAM", OsString::from("ghostty")),
            ("TERM_PROGRAM_VERSION", OsString::from("1.2.3")),
            ("UNRELATED", OsString::from("ignored")),
        ];

        let pairs = host_terminal_env_pairs_from(lookup_from(&entries));

        assert_eq!(
            pairs,
            vec![
                ("TERM".to_owned(), "xterm-kitty".to_owned()),
                ("COLORTERM".to_owned(), "truecolor".to_owned()),
                ("TERM_PROGRAM".to_owned(), "ghostty".to_owned()),
                ("TERM_PROGRAM_VERSION".to_owned(), "1.2.3".to_owned()),
            ]
        );
    }

    #[test]
    fn terminal_env_skips_empty_unrelated_and_typo_values() {
        let entries = [
            ("TERM", OsString::from("")),
            ("COLORTERM", OsString::from("truecolor")),
            ("TERM_PROGGRAM_VERSION", OsString::from("typo")),
            ("OTHER", OsString::from("value")),
        ];

        let pairs = host_terminal_env_pairs_from(lookup_from(&entries));

        assert_eq!(
            pairs,
            vec![("COLORTERM".to_owned(), "truecolor".to_owned())]
        );
        assert!(pairs.iter().all(|(key, _)| key != "TERM_PROGGRAM_VERSION"));
    }

    #[cfg(unix)]
    #[test]
    fn terminal_env_skips_non_utf8_values() {
        use std::os::unix::ffi::OsStringExt;

        let entries = [
            ("TERM", OsString::from_vec(vec![b'x', 0xff])),
            ("TERM_PROGRAM", OsString::from("WezTerm")),
        ];

        let pairs = host_terminal_env_pairs_from(lookup_from(&entries));

        assert_eq!(
            pairs,
            vec![("TERM_PROGRAM".to_owned(), "WezTerm".to_owned())]
        );
    }
}
