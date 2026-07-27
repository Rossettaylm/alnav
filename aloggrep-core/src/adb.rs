use std::process::Command;

use crate::live::{query_marker, spawn_session, LiveSession};

fn clock_command(device: Option<&str>) -> Command {
    let mut command = Command::new("adb");
    if let Some(serial) = device {
        command.arg("-s").arg(serial);
    }
    command.arg("shell").arg("date").arg("+%m-%d %H:%M:%S");
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
        assert_eq!(
            clock.get_args().collect::<Vec<_>>(),
            ["-s", "SERIAL", "shell", "date", "+%m-%d %H:%M:%S"]
        );
        assert_eq!(
            capture.get_args().collect::<Vec<_>>(),
            ["-s", "SERIAL", "logcat", "-v", "threadtime"]
        );
    }

    #[test]
    fn adb_commands_omit_selector_without_device() {
        let capture = capture_command(None);
        assert_eq!(
            capture.get_args().collect::<Vec<_>>(),
            ["logcat", "-v", "threadtime"]
        );
    }
}
