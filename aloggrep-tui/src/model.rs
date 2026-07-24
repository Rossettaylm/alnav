use std::sync::OnceLock;

use aloggrep::crash::CrashDetector;
use aloggrep::parser::{Level, LogEntry};

fn crash_detector() -> &'static CrashDetector {
    static DETECTOR: OnceLock<CrashDetector> = OnceLock::new();
    DETECTOR.get_or_init(CrashDetector::new)
}

/// Severe = level E/F, or a crash signature in the message (H2 jump target).
pub fn is_severe_row(row: &EntryRow) -> bool {
    matches!(row.level, Level::E | Level::F)
        || crash_detector().detect(&row.as_log_entry()).is_some()
}

#[derive(Debug, Clone)]
pub struct EntryRow {
    pub raw: String,
    /// Monotonic ingest id assigned by [`crate::app::App`] (M2 bookmarks).
    pub row_id: u64,
    pub timestamp: String,
    pub pid: String,
    pub tid: String,
    pub level: Level,
    pub tag: String,
    pub pkg: String,
    pub msg: String,
    /// Pre-computed at ingest time by [`crate::app::App::push_row`].
    /// True when level is E/F or the message matches a crash signature.
    /// Avoids calling CrashDetector on every minimap/find-severe scan.
    pub severe: bool,
}

impl EntryRow {
    /// Parse `line`; returns `None` for lines that don't match any known
    /// log format (they are dropped, same as the CLI's default no-multiline
    /// behavior — see design doc "数据模型" section).
    ///
    /// `row_id` is left `0`; [`crate::app::App`] assigns a real id on ingest.
    pub fn from_line(line: &str) -> Option<Self> {
        let entry = LogEntry::parse(line)?;
        Some(EntryRow {
            raw: line.to_string(),
            row_id: 0,
            timestamp: entry.timestamp.to_string(),
            pid: entry.pid.to_string(),
            tid: entry.tid.to_string(),
            level: entry.level,
            tag: entry.tag.to_string(),
            pkg: entry.pkg.to_string(),
            msg: entry.msg.to_string(),
            severe: false, // set by App::push_row via is_severe_row()
        })
    }

    /// File/mmap path: parse when possible; otherwise keep a raw-only row so
    /// unparseable lines remain browsable (stream ingest still drops them).
    pub fn from_line_or_raw(line: &str) -> Self {
        if let Some(row) = Self::from_line(line) {
            return row;
        }
        EntryRow {
            raw: line.to_string(),
            row_id: 0,
            timestamp: String::new(),
            pid: String::new(),
            tid: String::new(),
            level: Level::I,
            tag: String::new(),
            pkg: String::new(),
            msg: line.to_string(),
            severe: false,
        }
    }

    /// Borrow this row's owned fields as a `LogEntry`, so `aloggrep_core`
    /// matching code (`Expr::matches`, `LogEntry::time_hms`/`time_full`)
    /// can be reused without duplicating logic.
    pub fn as_log_entry(&self) -> LogEntry<'_> {
        LogEntry {
            timestamp: &self.timestamp,
            pid: &self.pid,
            tid: &self.tid,
            level: self.level,
            tag: &self.tag,
            pkg: &self.pkg,
            msg: &self.msg,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_line_parses_threadtime() {
        let row = EntryRow::from_line("04-02 10:00:00.000  1234  5678 E MyTag   : boom").unwrap();
        assert_eq!(row.tag, "MyTag");
        assert_eq!(row.msg, "boom");
        assert_eq!(row.level, Level::E);
        assert_eq!(row.pid, "1234");
        assert_eq!(row.tid, "5678");
    }

    #[test]
    fn test_from_line_rejects_unparseable() {
        assert!(EntryRow::from_line("not a log line at all").is_none());
    }

    #[test]
    fn test_from_line_or_raw_keeps_unparseable() {
        let row = EntryRow::from_line_or_raw("not a log line at all");
        assert_eq!(row.raw, "not a log line at all");
        assert_eq!(row.msg, "not a log line at all");
        assert!(row.tag.is_empty());
        let parsed = EntryRow::from_line_or_raw("04-02 10:00:00.000  1  1 I TagA   : ok");
        assert_eq!(parsed.tag, "TagA");
    }

    #[test]
    fn test_as_log_entry_roundtrips_for_matching() {
        let row = EntryRow::from_line("04-02 10:00:00.000  1234  5678 E MyTag   : boom").unwrap();
        let entry = row.as_log_entry();
        assert_eq!(entry.tag, "MyTag");
        assert_eq!(entry.msg, "boom");
    }
}
