use std::io::{self, BufRead, BufReader, Lines};
use std::process::{Child, ChildStdout, Command, Stdio};

use crate::parser::LogEntry;

/// Query the device's current time as `MM-DD HH:MM:SS`, matching the prefix
/// `LogEntry::time_full()` extracts from hilog lines. Used by `--hdc` to skip
/// hilogd's buffered history and start from "now". Returns `None` if the
/// query fails, in which case `--hdc` falls back to showing whatever hilog
/// dumps on start.
pub fn now_marker(device: Option<&str>) -> Option<String> {
    let mut cmd = Command::new("hdc");
    if let Some(serial) = device {
        cmd.arg("-t").arg(serial);
    }
    cmd.arg("shell").arg("date").arg("+%m-%d %H:%M:%S");
    let output = cmd.output().ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout).ok().map(|s| s.trim().to_string())
}

/// Wraps a raw hilog line iterator and drops lines older than `start_marker`
/// (device-clock "MM-DD HH:MM:SS"). `hdc hilog` dumps hilogd's buffered
/// history before streaming live, which floods `--hdc` with stale entries;
/// this restores "only what happens from now on" semantics without touching
/// the shared device-side ring buffer (so other readers, e.g. a persistent
/// capture daemon, are unaffected).
pub struct HdcLiveFilter<I> {
    pub inner: I,
    pub start_marker: Option<String>,
}

impl<I: Iterator<Item = io::Result<String>>> Iterator for HdcLiveFilter<I> {
    type Item = io::Result<String>;

    fn next(&mut self) -> Option<Self::Item> {
        let Some(marker) = &self.start_marker else {
            return self.inner.next();
        };
        for line in self.inner.by_ref() {
            let l = match &line {
                Ok(l) => l,
                Err(_) => continue,
            };
            let is_live = LogEntry::parse(l)
                .and_then(|e| e.time_full().map(str::to_string))
                .map_or(false, |t| t.as_str() >= marker.as_str());
            if is_live {
                self.start_marker = None;
                return Some(line);
            }
        }
        None
    }
}

/// Concrete line source produced by a spawned `hdc hilog` child.
pub type HdcLines = HdcLiveFilter<Lines<BufReader<ChildStdout>>>;

/// A running `hdc hilog` capture: the child process (caller must `kill`+`wait`
/// it on shutdown) and the filtered line iterator over its stdout.
pub struct HdcSession {
    pub child: Child,
    pub lines: HdcLines,
    /// `true` if the device-time query failed, meaning `lines` will include
    /// hilogd's buffered history instead of only live entries. Callers decide
    /// how to surface this (CLI prints a warning; a TUI might show a status line).
    pub used_history_fallback: bool,
}

/// Spawn `hdc [-t SERIAL] hilog --no-block` and wrap its stdout so only lines
/// at or after "now" are yielded (see `HdcLiveFilter`).
pub fn spawn_hilog(device: Option<&str>) -> Result<HdcSession, String> {
    let start_marker = now_marker(device);
    let used_history_fallback = start_marker.is_none();

    let mut cmd = Command::new("hdc");
    if let Some(serial) = device {
        cmd.arg("-t").arg(serial);
    }
    cmd.arg("hilog").arg("--no-block");
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let mut child = cmd.spawn().map_err(|e| {
        if e.kind() == io::ErrorKind::NotFound {
            "hdc not found, please install HDC tools".to_string()
        } else {
            format!("failed to start 'hdc hilog': {e}")
        }
    })?;

    let child_stdout = child.stdout.take().expect("piped stdout");
    let lines = HdcLiveFilter {
        inner: BufReader::new(child_stdout).lines(),
        start_marker,
    };

    Ok(HdcSession { child, lines, used_history_fallback })
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
