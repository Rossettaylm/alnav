use std::io;
use std::process::Command;

use crate::live::{query_marker, spawn_live_tail};

pub use crate::live::{
    LiveFilter as HdcLiveFilter, LiveLines as HdcLines, LiveSession as HdcSession,
};

/// Single `hdc shell` script so a format string with a space stays one token
/// on the device shell (same pitfall as ADB + toybox `date`).
const CLOCK_SHELL: &str = "date '+%m-%d %H:%M:%S'";

fn clock_command(device: Option<&str>) -> Command {
    let mut command = Command::new("hdc");
    if let Some(serial) = device {
        command.arg("-t").arg(serial);
    }
    command.arg("shell").arg(CLOCK_SHELL);
    command
}

fn ping_command(device: Option<&str>) -> Command {
    let mut command = Command::new("hdc");
    if let Some(serial) = device {
        command.arg("-t").arg(serial);
    }
    command.arg("shell").arg("echo").arg("ok");
    command
}

/// Types the default `hilog` follow reads. Cleared before follow so the reader
/// starts on an empty ring instead of replaying hilogd history.
const CLEAR_TYPES: &str = "app,core,init,only_prerelease";

fn hdc_command(device: Option<&str>) -> Command {
    let mut command = Command::new("hdc");
    if let Some(serial) = device {
        command.arg("-t").arg(serial);
    }
    command
}

fn capture_command(device: Option<&str>) -> Command {
    let mut command = hdc_command(device);
    command.arg("shell").arg("hilog");
    command
}

fn clear_command(device: Option<&str>) -> Command {
    let mut command = hdc_command(device);
    command
        .arg("shell")
        .arg("hilog")
        .arg("-r")
        .arg("-t")
        .arg(CLEAR_TYPES);
    command
}

fn clear_hilog_buffer(device: Option<&str>) -> Result<(), String> {
    let output = clear_command(device).output().map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            "hdc not found, please install HDC tools".to_string()
        } else {
            format!("failed to clear hilog buffer: {error}")
        }
    })?;
    if output.status.success() {
        return Ok(());
    }
    let detail = String::from_utf8_lossy(&output.stderr);
    let detail = detail.trim();
    let detail = if detail.is_empty() {
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    } else {
        detail.to_string()
    };
    if detail.is_empty() {
        Err("failed to clear hilog buffer".to_string())
    } else {
        Err(format!("failed to clear hilog buffer: {detail}"))
    }
}

/// True when a trivial `hdc shell echo ok` succeeds.
/// Independent of [`now_marker`]: clock-query failure must not be treated as
/// unreachable.
pub fn device_reachable(device: Option<&str>) -> bool {
    let Ok(output) = ping_command(device).output() else {
        return false;
    };
    output.status.success() && String::from_utf8_lossy(&output.stdout).trim() == "ok"
}

/// Query the device's current time as `MM-DD HH:MM:SS`, matching the prefix
/// `LogEntry::time_full()` extracts from hilog lines. Not used to start
/// `--hdc`: that path clears the in-memory ring and follows from empty.
pub fn now_marker(device: Option<&str>) -> Option<String> {
    query_marker(clock_command(device))
}

/// Clear hilogd's in-memory rings, then follow `hdc shell hilog`.
///
/// `hdc hilog` ignores every argument except `h` and always replays the
/// buffer. `hilog -r` drops app/core/init/only_prerelease in RAM (persisted
/// files under `/data/log/hilog` stay). The follow then blocks on an empty
/// ring, so startup does not transfer history. Lines between clear and follow
/// are dropped. Reconnect uses this same entry.
pub fn spawn_hilog(device: Option<&str>) -> Result<HdcSession, String> {
    clear_hilog_buffer(device)?;
    spawn_live_tail(
        capture_command(device),
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
        let ping = ping_command(Some("SERIAL"));
        assert_eq!(
            clock.get_args().collect::<Vec<_>>(),
            ["-t", "SERIAL", "shell", CLOCK_SHELL]
        );
        assert_eq!(
            capture.get_args().collect::<Vec<_>>(),
            ["-t", "SERIAL", "shell", "hilog"]
        );
        assert_eq!(
            clear_command(Some("SERIAL")).get_args().collect::<Vec<_>>(),
            [
                "-t",
                "SERIAL",
                "shell",
                "hilog",
                "-r",
                "-t",
                "app,core,init,only_prerelease"
            ]
        );
        assert_eq!(
            ping.get_args().collect::<Vec<_>>(),
            ["-t", "SERIAL", "shell", "echo", "ok"]
        );
        assert_eq!(
            capture_command(None).get_args().collect::<Vec<_>>(),
            ["shell", "hilog"]
        );
    }
}
