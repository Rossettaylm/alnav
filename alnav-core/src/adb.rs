use std::process::Command;

use crate::live::{query_marker, spawn_session, LiveSession};

/// Single `adb shell` script so the format string with a space is not re-split
/// by the device-side toybox `date` (which rejects more than one argument).
const CLOCK_SHELL: &str = "date '+%m-%d %H:%M:%S'";

fn clock_command(device: Option<&str>) -> Command {
    let mut command = Command::new("adb");
    if let Some(serial) = device {
        command.arg("-s").arg(serial);
    }
    command.arg("shell").arg(CLOCK_SHELL);
    command
}

fn state_command(device: Option<&str>) -> Command {
    let mut command = Command::new("adb");
    if let Some(serial) = device {
        command.arg("-s").arg(serial);
    }
    command.arg("get-state");
    command
}

fn capture_command(device: Option<&str>) -> Command {
    let mut command = Command::new("adb");
    if let Some(serial) = device {
        command.arg("-s").arg(serial);
    }
    command.arg("logcat").arg("-v").arg("threadtime");
    command
}

/// True when `adb get-state` reports `device` (online and authorized).
/// Independent of [`now_marker`]: clock-query failure must not be treated as
/// unreachable.
pub fn device_reachable(device: Option<&str>) -> bool {
    let Ok(output) = state_command(device).output() else {
        return false;
    };
    output.status.success() && String::from_utf8_lossy(&output.stdout).trim() == "device"
}

/// Query Android device time using the same marker format as threadtime logs.
pub fn now_marker(device: Option<&str>) -> Option<String> {
    query_marker(clock_command(device))
}

/// Spawn `adb [-s SERIAL] logcat -v threadtime`, skipping buffered records
/// older than the device time captured immediately before startup.
pub fn spawn_logcat(device: Option<&str>) -> Result<LiveSession, String> {
    let start_marker = now_marker(device);
    spawn_session(
        capture_command(device),
        start_marker,
        "adb not found, please install Android platform-tools",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adb_commands_use_s_selector_and_threadtime() {
        let clock = clock_command(Some("SERIAL"));
        let capture = capture_command(Some("SERIAL"));
        let state = state_command(Some("SERIAL"));
        assert_eq!(
            clock.get_args().collect::<Vec<_>>(),
            ["-s", "SERIAL", "shell", CLOCK_SHELL]
        );
        assert_eq!(
            capture.get_args().collect::<Vec<_>>(),
            ["-s", "SERIAL", "logcat", "-v", "threadtime"]
        );
        assert_eq!(
            state.get_args().collect::<Vec<_>>(),
            ["-s", "SERIAL", "get-state"]
        );
    }

    #[test]
    fn adb_commands_omit_selector_without_device() {
        let capture = capture_command(None);
        assert_eq!(
            capture.get_args().collect::<Vec<_>>(),
            ["logcat", "-v", "threadtime"]
        );
        assert_eq!(
            clock_command(None).get_args().collect::<Vec<_>>(),
            ["shell", CLOCK_SHELL]
        );
    }
}
