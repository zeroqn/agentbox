use std::fmt::Write as _;
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

const TERMINAL_TRACE_ENV: &str = "LOFTD_TERMINAL_TRACE";

const TRACE_HIT_LIMIT: usize = 24;

#[derive(Debug, Clone, Copy)]
struct TerminalPattern {
    name: &'static str,
    bytes: &'static [u8],
}

const TERMINAL_PATTERNS: &[TerminalPattern] = &[
    TerminalPattern {
        name: "full_mode_reassert",
        bytes: b"\x1b[?2026h\x1b[?1049h\x1b[2J\x1b[?1000h\x1b[?1002h\x1b[?1003h\x1b[?1006h\x1b[?2004h\x1b[?1004h\x1b[?2026l",
    },
    TerminalPattern {
        name: "periodic_mode_reassert",
        bytes: b"\x1b[?1000h\x1b[?1002h\x1b[?1003h\x1b[?1006h\x1b[?2004h",
    },
    TerminalPattern {
        name: "alt_enter",
        bytes: b"\x1b[?1049h",
    },
    TerminalPattern {
        name: "alt_exit",
        bytes: b"\x1b[?1049l",
    },
    TerminalPattern {
        name: "clear_screen_2j",
        bytes: b"\x1b[2J",
    },
    TerminalPattern {
        name: "clear_screen_j",
        bytes: b"\x1b[J",
    },
    TerminalPattern {
        name: "cursor_home",
        bytes: b"\x1b[H",
    },
    TerminalPattern {
        name: "sync_begin",
        bytes: b"\x1b[?2026h",
    },
    TerminalPattern {
        name: "sync_end",
        bytes: b"\x1b[?2026l",
    },
    TerminalPattern {
        name: "focus_report_enable",
        bytes: b"\x1b[?1004h",
    },
    TerminalPattern {
        name: "focus_report_disable",
        bytes: b"\x1b[?1004l",
    },
    TerminalPattern {
        name: "focus_gained_input",
        bytes: b"\x1b[I",
    },
    TerminalPattern {
        name: "focus_lost_input",
        bytes: b"\x1b[O",
    },
    TerminalPattern {
        name: "paste_enable",
        bytes: b"\x1b[?2004h",
    },
    TerminalPattern {
        name: "paste_disable",
        bytes: b"\x1b[?2004l",
    },
    TerminalPattern {
        name: "mouse_1000_enable",
        bytes: b"\x1b[?1000h",
    },
    TerminalPattern {
        name: "mouse_1000_disable",
        bytes: b"\x1b[?1000l",
    },
    TerminalPattern {
        name: "mouse_1002_enable",
        bytes: b"\x1b[?1002h",
    },
    TerminalPattern {
        name: "mouse_1002_disable",
        bytes: b"\x1b[?1002l",
    },
    TerminalPattern {
        name: "mouse_1003_enable",
        bytes: b"\x1b[?1003h",
    },
    TerminalPattern {
        name: "mouse_1003_disable",
        bytes: b"\x1b[?1003l",
    },
    TerminalPattern {
        name: "mouse_1006_enable",
        bytes: b"\x1b[?1006h",
    },
    TerminalPattern {
        name: "mouse_1006_disable",
        bytes: b"\x1b[?1006l",
    },
];

#[derive(Debug, Clone, PartialEq, Eq)]
struct TerminalSequenceSummary {
    hits: Vec<TerminalSequenceHit>,
}

impl TerminalSequenceSummary {
    fn summarize(bytes: &[u8]) -> Self {
        let mut hits = Vec::new();
        for pattern in TERMINAL_PATTERNS {
            collect_hits(bytes, *pattern, &mut hits);
        }
        hits.sort_by_key(|hit| hit.offset);
        Self { hits }
    }

    fn is_empty(&self) -> bool {
        self.hits.is_empty()
    }

    fn count(&self, name: &str) -> usize {
        self.hits.iter().filter(|hit| hit.name == name).count()
    }

    fn render(&self) -> String {
        if self.is_empty() {
            return "none".to_owned();
        }
        let mut rendered = String::new();
        let mut first = true;
        for pattern in TERMINAL_PATTERNS {
            let count = self.count(pattern.name);
            if count == 0 {
                continue;
            }
            if !first {
                rendered.push(' ');
            }
            first = false;
            let _ = write!(rendered, "{}={}", pattern.name, count);
        }
        rendered.push_str(" hits=");
        for (index, hit) in self.hits.iter().take(TRACE_HIT_LIMIT).enumerate() {
            if index > 0 {
                rendered.push(',');
            }
            let _ = write!(rendered, "{}@{}", hit.name, hit.offset);
        }
        if self.hits.len() > TRACE_HIT_LIMIT {
            let _ = write!(rendered, ",...+{}", self.hits.len() - TRACE_HIT_LIMIT);
        }
        rendered
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TerminalSequenceHit {
    name: &'static str,
    offset: usize,
}

pub fn terminal_trace_env_pair_from_process_env() -> Option<(String, String)> {
    std::env::var(TERMINAL_TRACE_ENV)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(|value| {
            let path = Path::new(&value);
            let propagated = if path.is_absolute() {
                path.to_path_buf()
            } else {
                std::env::current_dir()
                    .unwrap_or_else(|_| std::path::PathBuf::from("."))
                    .join(path)
            };
            (
                TERMINAL_TRACE_ENV.to_owned(),
                propagated.to_string_lossy().into_owned(),
            )
        })
}

pub fn trace_data_from_env(role: &str, direction: &str, bytes: &[u8]) {
    if std::env::var_os(TERMINAL_TRACE_ENV).is_none() {
        return;
    }
    let summary = TerminalSequenceSummary::summarize(bytes);
    trace_event_from_env(
        role,
        "data",
        &format!(
            "direction={direction} bytes={} sequences={}",
            bytes.len(),
            summary.render()
        ),
    );
}

pub fn trace_event_from_env(role: &str, event: &str, detail: &str) {
    let Some(path) = std::env::var_os(TERMINAL_TRACE_ENV) else {
        return;
    };
    if path.is_empty() {
        return;
    }
    let _ = append_terminal_trace_event(Path::new(&path), role, event, detail);
}

fn append_terminal_trace_event(
    path: &Path,
    role: &str,
    event: &str,
    detail: &str,
) -> io::Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(
        file,
        "ts_us={} role={} event={} {}",
        unix_timestamp_micros(),
        sanitize_field(role),
        sanitize_field(event),
        sanitize_detail(detail)
    )
}

fn collect_hits(bytes: &[u8], pattern: TerminalPattern, hits: &mut Vec<TerminalSequenceHit>) {
    if pattern.bytes.is_empty() || bytes.len() < pattern.bytes.len() {
        return;
    }
    let mut offset = 0;
    while offset + pattern.bytes.len() <= bytes.len() {
        if &bytes[offset..offset + pattern.bytes.len()] == pattern.bytes {
            hits.push(TerminalSequenceHit {
                name: pattern.name,
                offset,
            });
            offset += pattern.bytes.len();
        } else {
            offset += 1;
        }
    }
}

fn unix_timestamp_micros() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros()
}

fn sanitize_field(value: &str) -> String {
    value
        .chars()
        .map(|ch| match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '_' | '-' => ch,
            _ => '_',
        })
        .collect()
}

fn sanitize_detail(value: &str) -> String {
    value
        .chars()
        .map(|ch| if ch == '\n' || ch == '\r' { ' ' } else { ch })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summarizes_alt_screen_clear_focus_and_periodic_reassert() {
        let bytes =
            b"x\x1b[?1049h\x1b[2J\x1b[I\x1b[?1000h\x1b[?1002h\x1b[?1003h\x1b[?1006h\x1b[?2004hy";

        let summary = TerminalSequenceSummary::summarize(bytes);

        assert_eq!(summary.count("alt_enter"), 1);
        assert_eq!(summary.count("clear_screen_2j"), 1);
        assert_eq!(summary.count("focus_gained_input"), 1);
        assert_eq!(summary.count("periodic_mode_reassert"), 1);
        assert_eq!(summary.count("mouse_1006_enable"), 1);
        assert!(summary.render().contains("alt_enter=1"));
        assert!(summary.render().contains("hits="));
    }

    #[test]
    fn empty_summary_renders_none() {
        let summary = TerminalSequenceSummary::summarize(b"plain text");

        assert!(summary.is_empty());
        assert_eq!(summary.render(), "none");
    }

    #[test]
    fn trace_line_sanitizes_role_event_and_detail() {
        let path = std::env::temp_dir().join(format!(
            "loftd-terminal-trace-test-{}.log",
            unix_timestamp_micros()
        ));

        append_terminal_trace_event(&path, "host x", "data/y", "a\nb").unwrap();

        let trace = std::fs::read_to_string(&path).unwrap();
        let _ = std::fs::remove_file(path);
        assert!(trace.contains("role=host_x"));
        assert!(trace.contains("event=data_y"));
        assert!(trace.contains("a b"));
    }

    #[test]
    fn propagated_trace_env_makes_relative_paths_absolute() {
        let old = std::env::var_os(TERMINAL_TRACE_ENV);
        unsafe {
            std::env::set_var(TERMINAL_TRACE_ENV, "relative-trace.log");
        }

        let pair = terminal_trace_env_pair_from_process_env().unwrap();

        if let Some(old) = old {
            unsafe { std::env::set_var(TERMINAL_TRACE_ENV, old) };
        } else {
            unsafe { std::env::remove_var(TERMINAL_TRACE_ENV) };
        }
        assert_eq!(pair.0, TERMINAL_TRACE_ENV);
        assert!(Path::new(&pair.1).is_absolute());
        assert!(pair.1.ends_with("relative-trace.log"));
    }
}
