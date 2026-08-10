use std::io::{self, BufRead, BufReader, Lines};
use std::process::{Child, ChildStdout, Command, Stdio};

use crate::parser::LogEntry;

/// Drops buffered device logs older than the marker, then passes every
/// subsequent line (including unparsed continuation lines) through unchanged.
pub struct LiveFilter<I> {
    pub inner: I,
    pub start_marker: Option<String>,
}

impl<I: Iterator<Item = io::Result<String>>> Iterator for LiveFilter<I> {
    type Item = io::Result<String>;

    fn next(&mut self) -> Option<Self::Item> {
        let Some(marker) = &self.start_marker else {
            return self.inner.next();
        };
        for line in self.inner.by_ref() {
            let value = match &line {
                Ok(value) => value,
                Err(_) => continue,
            };
            let is_live = LogEntry::parse(value)
                .and_then(|entry| entry.time_full().map(str::to_string))
                .is_some_and(|time| time.as_str() >= marker.as_str());
            if is_live {
                self.start_marker = None;
                return Some(line);
            }
        }
        None
    }
}

/// Concrete line source produced by a live device-log child process.
pub type LiveLines = LiveFilter<Lines<BufReader<ChildStdout>>>;

/// Running live capture shared by the core CLI and TUI.
pub struct LiveSession {
    pub child: Child,
    pub lines: LiveLines,
    pub used_history_fallback: bool,
}

pub(crate) fn query_marker(mut command: Command) -> Option<String> {
    let output = command.output().ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout)
        .ok()
        .map(|value| value.trim().to_string())
}

pub(crate) fn spawn_session(
    mut command: Command,
    start_marker: Option<String>,
    missing_tool_message: &str,
) -> Result<LiveSession, String> {
    let used_history_fallback = start_marker.is_none();
    command.stdout(Stdio::piped());
    // Discard stderr: a piped-but-unread stderr fills (~64KiB) and can
    // deadlock the capture child so stdout never EOFs / never yields lines.
    command.stderr(Stdio::null());

    let mut child = command.spawn().map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            missing_tool_message.to_string()
        } else {
            format!("failed to start device log capture: {error}")
        }
    })?;
    let child_stdout = child.stdout.take().expect("piped stdout");
    let lines = LiveFilter {
        inner: BufReader::new(child_stdout).lines(),
        start_marker,
    };

    Ok(LiveSession {
        child,
        lines,
        used_history_fallback,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skips_buffered_lines_then_preserves_continuations() {
        let lines: Vec<io::Result<String>> = vec![
            Ok("04-02 09:59:59.000  1234  5678 I Tag     : old".into()),
            Ok("04-02 10:00:00.000  1234  5678 E Tag     : live".into()),
            Ok("    at some.stack.trace(File.java:10)".into()),
        ];
        let filter = LiveFilter {
            inner: lines.into_iter(),
            start_marker: Some("04-02 10:00:00".into()),
        };

        let output: Vec<String> = filter.map(|line| line.unwrap()).collect();
        assert_eq!(output.len(), 2);
        assert!(output[0].contains("live"));
        assert!(output[1].contains("stack.trace"));
    }
}
