use std::process::Command;

use crate::live::{query_marker, spawn_session};

pub use crate::live::{
    LiveFilter as HdcLiveFilter, LiveLines as HdcLines, LiveSession as HdcSession,
};

fn clock_command(device: Option<&str>) -> Command {
    let mut command = Command::new("hdc");
    if let Some(serial) = device {
        command.arg("-t").arg(serial);
    }
    command.arg("shell").arg("date").arg("+%m-%d %H:%M:%S");
    command
}

fn capture_command(device: Option<&str>) -> Command {
    let mut command = Command::new("hdc");
    if let Some(serial) = device {
        command.arg("-t").arg(serial);
    }
    command.arg("hilog").arg("--no-block");
    command
}

/// Query the device's current time as `MM-DD HH:MM:SS`, matching the prefix
/// `LogEntry::time_full()` extracts from hilog lines. Used by `--hdc` to skip
/// hilogd's buffered history and start from "now". Returns `None` if the
/// query fails, in which case `--hdc` falls back to showing whatever hilog
/// dumps on start.
pub fn now_marker(device: Option<&str>) -> Option<String> {
    query_marker(clock_command(device))
}

/// Spawn `hdc [-t SERIAL] hilog --no-block` and wrap its stdout so only lines
/// at or after "now" are yielded (see `HdcLiveFilter`).
pub fn spawn_hilog(device: Option<&str>) -> Result<HdcSession, String> {
    let start_marker = now_marker(device);
    spawn_session(
        capture_command(device),
        start_marker,
        "hdc not found, please install HDC tools",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;

    #[test]
    fn test_hdc_live_filter_skips_lines_before_marker() {
        let lines: Vec<io::Result<String>> = vec![
            Ok("04-02 09:59:58.000  1234  5678 I Tag     : old boot log".to_string()),
            Ok("04-02 09:59:59.000  1234  5678 I Tag     : also old".to_string()),
            Ok("04-02 10:00:00.000  1234  5678 I Tag     : right at marker".to_string()),
            Ok("04-02 10:00:01.000  1234  5678 E Tag     : live entry".to_string()),
        ];
        let filter = HdcLiveFilter {
            inner: lines.into_iter(),
            start_marker: Some("04-02 10:00:00".to_string()),
        };
        let out: Vec<String> = filter.map(|l| l.unwrap()).collect();
        assert_eq!(out.len(), 2);
        assert!(out[0].contains("right at marker"));
        assert!(out[1].contains("live entry"));
    }

    #[test]
    fn test_hdc_live_filter_passes_unparsed_lines_once_started() {
        let lines: Vec<io::Result<String>> = vec![
            Ok("========Zeroth log of type: init".to_string()),
            Ok("04-02 10:00:01.000  1234  5678 E Tag     : live entry".to_string()),
            Ok("    at some.stack.trace(File.java:10)".to_string()),
        ];
        let filter = HdcLiveFilter {
            inner: lines.into_iter(),
            start_marker: Some("04-02 10:00:00".to_string()),
        };
        let out: Vec<String> = filter.map(|l| l.unwrap()).collect();
        assert_eq!(out.len(), 2);
        assert!(out[0].contains("live entry"));
        assert!(out[1].contains("stack.trace"));
    }

    #[test]
    fn test_hdc_live_filter_no_marker_passes_everything() {
        let lines: Vec<io::Result<String>> = vec![
            Ok("04-02 09:00:00.000  1234  5678 I Tag     : old boot log".to_string()),
            Ok("anything".to_string()),
        ];
        let filter = HdcLiveFilter {
            inner: lines.into_iter(),
            start_marker: None,
        };
        let out: Vec<String> = filter.map(|l| l.unwrap()).collect();
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn hdc_commands_use_t_device_selector() {
        let clock = clock_command(Some("SERIAL"));
        let capture = capture_command(Some("SERIAL"));
        assert_eq!(
            clock.get_args().collect::<Vec<_>>(),
            ["-t", "SERIAL", "shell", "date", "+%m-%d %H:%M:%S"]
        );
        assert_eq!(
            capture.get_args().collect::<Vec<_>>(),
            ["-t", "SERIAL", "hilog", "--no-block"]
        );
    }
}
