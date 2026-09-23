//! Parent-observed managed guest exit marker.

use anyhow::{Context, Result};
use std::fs;
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

const MARKER_FILE: &str = "managed-exit.status";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ManagedExitObservation {
    ObservedGuestExit(i32),
    NoObservedGuestExit,
    ObservedGuestExitDifferentCode { expected: i32, observed: i32 },
    InvalidMarker(String),
}

pub(crate) fn reset_observed_guest_exit(task_state_dir: &Path) -> Result<()> {
    let marker_path = marker_path(task_state_dir);
    match fs::remove_file(&marker_path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err).with_context(|| {
            format!(
                "failed to reset managed guest exit marker '{}'",
                marker_path.display()
            )
        }),
    }
}

pub(crate) fn write_observed_guest_exit(task_state_dir: &Path, code: i32) -> Result<()> {
    let marker_path = marker_path(task_state_dir);
    let temp_path = task_state_dir.join(format!(".{MARKER_FILE}.{}.tmp", std::process::id()));
    fs::write(&temp_path, format!("{code}\n")).with_context(|| {
        format!(
            "failed to write managed guest exit marker '{}'",
            temp_path.display()
        )
    })?;
    fs::rename(&temp_path, &marker_path).with_context(|| {
        format!(
            "failed to publish managed guest exit marker '{}'",
            marker_path.display()
        )
    })?;
    Ok(())
}

pub(crate) fn read_observed_guest_exit(task_state_dir: &Path) -> ManagedExitObservation {
    match fs::read_to_string(marker_path(task_state_dir)) {
        Ok(contents) => parse_observed_guest_exit(&contents),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            ManagedExitObservation::NoObservedGuestExit
        }
        Err(err) => ManagedExitObservation::InvalidMarker(err.to_string()),
    }
}

pub(crate) fn read_matching_observed_guest_exit(
    task_state_dir: &Path,
    expected: i32,
) -> ManagedExitObservation {
    match read_observed_guest_exit(task_state_dir) {
        ManagedExitObservation::ObservedGuestExit(observed) if observed == expected => {
            ManagedExitObservation::ObservedGuestExit(observed)
        }
        ManagedExitObservation::ObservedGuestExit(observed) => {
            ManagedExitObservation::ObservedGuestExitDifferentCode { expected, observed }
        }
        other => other,
    }
}

pub(crate) fn wait_for_matching_observed_guest_exit(
    task_state_dir: &Path,
    expected: i32,
    timeout: Duration,
    poll: Duration,
) -> ManagedExitObservation {
    let deadline = Instant::now() + timeout;
    loop {
        let observation = read_matching_observed_guest_exit(task_state_dir, expected);
        if observation != ManagedExitObservation::NoObservedGuestExit || Instant::now() >= deadline
        {
            return observation;
        }
        thread::sleep(poll.min(deadline.saturating_duration_since(Instant::now())));
    }
}

pub(crate) fn observed_guest_exit_code(task_state_dir: &Path) -> Option<i32> {
    match read_observed_guest_exit(task_state_dir) {
        ManagedExitObservation::ObservedGuestExit(code) => Some(code),
        _ => None,
    }
}

fn parse_observed_guest_exit(contents: &str) -> ManagedExitObservation {
    match contents.trim().parse::<i32>() {
        Ok(code) => ManagedExitObservation::ObservedGuestExit(code),
        Err(err) => ManagedExitObservation::InvalidMarker(err.to_string()),
    }
}

fn marker_path(task_state_dir: &Path) -> std::path::PathBuf {
    task_state_dir.join(MARKER_FILE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_round_trips_parent_observed_guest_exit() {
        let temp = tempfile::tempdir().expect("tempdir");

        write_observed_guest_exit(temp.path(), 127).expect("write marker");

        assert_eq!(
            read_observed_guest_exit(temp.path()),
            ManagedExitObservation::ObservedGuestExit(127)
        );
        assert_eq!(
            read_matching_observed_guest_exit(temp.path(), 127),
            ManagedExitObservation::ObservedGuestExit(127)
        );
    }

    #[test]
    fn marker_reset_removes_stale_parent_observation() {
        let temp = tempfile::tempdir().expect("tempdir");

        write_observed_guest_exit(temp.path(), 130).expect("write marker");
        reset_observed_guest_exit(temp.path()).expect("reset marker");

        assert_eq!(
            read_observed_guest_exit(temp.path()),
            ManagedExitObservation::NoObservedGuestExit
        );
    }

    #[test]
    fn marker_reports_missing_different_and_invalid_states() {
        let temp = tempfile::tempdir().expect("tempdir");

        assert_eq!(
            read_matching_observed_guest_exit(temp.path(), 127),
            ManagedExitObservation::NoObservedGuestExit
        );

        write_observed_guest_exit(temp.path(), 126).expect("write marker");
        assert_eq!(
            read_matching_observed_guest_exit(temp.path(), 127),
            ManagedExitObservation::ObservedGuestExitDifferentCode {
                expected: 127,
                observed: 126
            }
        );

        fs::write(marker_path(temp.path()), "not-a-code\n").expect("invalid marker");
        assert!(matches!(
            read_matching_observed_guest_exit(temp.path(), 127),
            ManagedExitObservation::InvalidMarker(_)
        ));
    }
}
