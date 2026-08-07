use std::borrow::Cow;
use std::sync::OnceLock;

use alnav::crash::CrashDetector;
use alnav::parser::{Level, LogEntry};

fn crash_detector() -> &'static CrashDetector {
    static DETECTOR: OnceLock<CrashDetector> = OnceLock::new();
    DETECTOR.get_or_init(CrashDetector::new)
}

/// Severe = level E/F, or a crash signature in the message (H2 jump target).
pub fn is_severe_row(row: &EntryRow) -> bool {
    matches!(row.level, Level::E | Level::F)
        || crash_detector().detect(&row.as_log_entry()).is_some()
}

/// Control chars that must not reach the terminal (keep TAB).
fn is_control_display(c: char) -> bool {
    c != '\t' && (c.is_control() || c == '\u{7f}')
}

/// True when a line is mostly binary / undecoded junk (cracked xlog chunks).
/// Light binary fields inside otherwise textual lines (e.g. sign blobs) stay
/// as text after [`sanitize_display_text`]; only heavy junk is folded.
fn is_binary_heavy(s: &str) -> bool {
    let n = s.chars().count();
    if n == 0 {
        return false;
    }
    let mut bad = 0usize;
    let mut printable_ascii = 0usize;
    for c in s.chars() {
        if is_control_display(c) || c == '\u{FFFD}' {
            bad += 1;
        } else if c == '\t' || c.is_ascii_graphic() || c == ' ' {
            printable_ascii += 1;
        }
    }
    if bad == 0 {
        return false;
    }
    let bad_ratio = bad as f64 / n as f64;
    let printable_ratio = printable_ascii as f64 / n as f64;
    (n <= 24 && bad >= 2) || (bad >= 3 && printable_ratio < 0.55) || bad_ratio >= 0.40
}

/// Strip terminal-hostile controls and UTF-8 replacement glyphs from display text.
fn sanitize_display_text(s: &str) -> String {
    s.chars()
        .filter(|c| *c != '\u{FFFD}' && !is_control_display(*c))
        .collect()
}

/// Prepare a raw log line for TUI ingest: fold binary-heavy lines, else strip
/// controls. Borrowed when no change is needed.
fn prepare_line_text(line: &str) -> Cow<'_, str> {
    if is_binary_heavy(line) {
        Cow::Owned(format!("[binary · {}B]", line.len()))
    } else if line
        .chars()
        .any(|c| is_control_display(c) || c == '\u{FFFD}')
    {
        Cow::Owned(sanitize_display_text(line))
    } else {
        Cow::Borrowed(line)
    }
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
    /// `true` when [`LogEntry::parse`] succeeded; `false` for file-mode raw
    /// fallback rows (still browsable when no filter is active).
    pub parsed: bool,
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
    /// Binary-heavy / control-laden input is normalized first so TUI rendering
    /// never feeds ESC/BEL/NUL (etc.) to the terminal.
    pub fn from_line(line: &str) -> Option<Self> {
        Self::parse_prepared(prepare_line_text(line))
    }

    /// File/mmap path: parse when possible; otherwise keep a raw-only row so
    /// unparseable lines remain browsable (stream ingest still drops them).
    /// Active filters reject these rows (CLI-aligned).
    pub fn from_line_or_raw(line: &str) -> Self {
        let line = prepare_line_text(line);
        if let Some(row) = Self::parse_prepared(Cow::Borrowed(line.as_ref())) {
            return row;
        }
        let owned = line.into_owned();
        EntryRow {
            raw: owned.clone(),
            row_id: 0,
            timestamp: String::new(),
            pid: String::new(),
            tid: String::new(),
            level: Level::I,
            tag: String::new(),
            pkg: String::new(),
            msg: owned,
            parsed: false,
            severe: false,
        }
    }

    fn parse_prepared(line: Cow<'_, str>) -> Option<Self> {
        let entry = LogEntry::parse(line.as_ref())?;
        // Copy borrowed fields before `line.into_owned()`.
        let timestamp = entry.timestamp.to_string();
        let pid = entry.pid.to_string();
        let tid = entry.tid.to_string();
        let level = entry.level;
        let tag = entry.tag.to_string();
        let pkg = entry.pkg.to_string();
        let msg = entry.msg.to_string();
        Some(EntryRow {
            raw: line.into_owned(),
            row_id: 0,
            timestamp,
            pid,
            tid,
            level,
            tag,
            pkg,
            msg,
            parsed: true,
            severe: false, // set by App::push_row via is_severe_row()
        })
    }

    /// Whether this row came from a successful log-format parse.
    pub fn is_parsed(&self) -> bool {
        self.parsed
    }

    /// Borrow this row's owned fields as a `LogEntry`, so `alnav-core`
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
        assert!(!row.is_parsed());
        let parsed = EntryRow::from_line_or_raw("04-02 10:00:00.000  1  1 I TagA   : ok");
        assert_eq!(parsed.tag, "TagA");
        assert!(parsed.is_parsed());
    }

    #[test]
    fn test_as_log_entry_roundtrips_for_matching() {
        let row = EntryRow::from_line("04-02 10:00:00.000  1234  5678 E MyTag   : boom").unwrap();
        let entry = row.as_log_entry();
        assert_eq!(entry.tag, "MyTag");
        assert_eq!(entry.msg, "boom");
    }

    #[test]
    fn test_sanitize_strips_controls_keeps_xlog() {
        let line = "2026-07-22 14:32:48.058|1[8028]8028|8028|I|Tag|hello\u{07}world";
        let row = EntryRow::from_line_or_raw(line);
        assert!(row.is_parsed());
        assert_eq!(row.msg, "helloworld");
        assert!(!row.raw.chars().any(is_control_display));
    }

    #[test]
    fn test_binary_heavy_line_folded() {
        // Cracked xlog chunk: controls dominate printable ASCII (real merge junk).
        let mut raw = String::from("\u{07}\0");
        for _ in 0..40 {
            raw.push('\0');
            raw.push(char::from(0x89));
            raw.push(char::from(0x03));
        }
        raw.push_str("EmojiRinit");
        let expected_len = raw.len();
        let row = EntryRow::from_line_or_raw(&raw);
        assert!(!row.is_parsed());
        assert_eq!(row.msg, format!("[binary · {expected_len}B]"));
    }

    #[test]
    fn test_light_binary_field_not_folded() {
        // Mostly printable with a small binary blob (sign-like).
        let line = format!(
            "2026-07-22 14:32:49.037|1[8028]8249|8028|I|MSF|sign= {}\u{FFFD}\u{FFFD}token=abc len=12",
            "x".repeat(40)
        );
        let row = EntryRow::from_line_or_raw(&line);
        assert!(row.is_parsed());
        assert!(
            !row.msg.starts_with("[binary · "),
            "light binary must not fold entire line: {:?}",
            row.msg
        );
        assert!(!row.msg.contains('\u{FFFD}'));
        assert!(row.msg.contains("token=abc"));
    }

    #[test]
    fn test_clean_xlog_unchanged() {
        let line = "2026-07-22 14:32:48.058|1[8028]8028|8028|I|startup_QLogInitTask|end";
        let row = EntryRow::from_line_or_raw(line);
        assert!(row.is_parsed());
        assert_eq!(row.raw, line);
        assert_eq!(row.tag, "startup_QLogInitTask");
        assert_eq!(row.msg, "end");
    }
}
