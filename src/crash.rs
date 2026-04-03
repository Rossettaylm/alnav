use regex::Regex;
use serde::Serialize;

use crate::parser::LogEntry;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CrashType {
    FatalException,
    Anr,
    NativeCrash,
}

#[derive(Debug, Serialize)]
pub struct CrashInfo {
    #[serde(rename = "type")]
    pub crash_type: CrashType,
    pub timestamp: String,
    pub pid: String,
    pub tid: String,
    pub tag: String,
    pub headline: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exception: Option<String>,
    pub stack: Vec<String>,
}

pub struct CrashDetector {
    fatal_re: Regex,
    anr_re: Regex,
    native_re: Regex,
    exception_re: Regex,
}

impl CrashDetector {
    pub fn new() -> Self {
        Self {
            fatal_re: Regex::new(r"(?i)FATAL EXCEPTION").unwrap(),
            anr_re: Regex::new(r"(?i)ANR in ").unwrap(),
            native_re: Regex::new(r"(?i)(SIGSEGV|SIGABRT|SIGBUS|SIGFPE|SIGILL|signal \d+\b)").unwrap(),
            exception_re: Regex::new(r"(?:Caused by: |\n)([a-zA-Z][\w.]*(?:Exception|Error|Throwable))").unwrap(),
        }
    }

    /// Check if the entry is a crash. Returns CrashType if detected.
    pub fn detect(&self, entry: &LogEntry) -> Option<CrashType> {
        if self.fatal_re.is_match(entry.msg) {
            Some(CrashType::FatalException)
        } else if self.anr_re.is_match(entry.msg) {
            Some(CrashType::Anr)
        } else if self.native_re.is_match(entry.msg) {
            Some(CrashType::NativeCrash)
        } else {
            None
        }
    }

    /// Parse a merged (multi-line) crash entry into structured CrashInfo.
    pub fn parse_crash(&self, entry: &LogEntry, crash_type: CrashType) -> CrashInfo {
        let msg = entry.msg;
        let lines: Vec<&str> = msg.lines().collect();
        let headline = lines.first().unwrap_or(&"").to_string();

        let exception = self.exception_re.captures(msg).map(|caps| {
            caps.get(1).unwrap().as_str().to_string()
        });

        let stack: Vec<String> = lines
            .iter()
            .filter(|line| {
                let t = line.trim();
                t.starts_with("at ") || t.starts_with("Caused by:")
            })
            .map(|line| line.trim().to_string())
            .collect();

        CrashInfo {
            crash_type,
            timestamp: entry.timestamp.trim().to_string(),
            pid: entry.pid.to_string(),
            tid: entry.tid.to_string(),
            tag: entry.tag.to_string(),
            headline,
            exception,
            stack,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::LogEntry;

    #[test]
    fn test_detect_fatal_exception() {
        let line = "04-02 12:34:56.789  1234  5678 E AndroidRuntime: FATAL EXCEPTION: main";
        let entry = LogEntry::parse(line).unwrap();
        let d = CrashDetector::new();
        assert!(matches!(d.detect(&entry), Some(CrashType::FatalException)));
    }

    #[test]
    fn test_detect_anr() {
        let line = "04-02 12:34:56.789  1234  5678 E ActivityManager: ANR in com.example.app";
        let entry = LogEntry::parse(line).unwrap();
        let d = CrashDetector::new();
        assert!(matches!(d.detect(&entry), Some(CrashType::Anr)));
    }

    #[test]
    fn test_detect_native() {
        let line = "04-02 12:34:56.789  1234  5678 F DEBUG   : signal 11 (SIGSEGV), code 1";
        let entry = LogEntry::parse(line).unwrap();
        let d = CrashDetector::new();
        assert!(matches!(d.detect(&entry), Some(CrashType::NativeCrash)));
    }

    #[test]
    fn test_no_crash() {
        let line = "04-02 12:34:56.789  1234  5678 W OkHttp  : timeout";
        let entry = LogEntry::parse(line).unwrap();
        let d = CrashDetector::new();
        assert!(d.detect(&entry).is_none());
    }

    #[test]
    fn test_parse_crash_with_stack() {
        let merged = "04-02 12:34:56.789  1234  5678 E AndroidRuntime: FATAL EXCEPTION: main\nProcess: com.example.app, PID: 1234\njava.lang.NullPointerException: Attempt to invoke\n\tat com.app.Foo.bar(Foo.java:12)\n\tat com.app.Baz.qux(Baz.java:34)\nCaused by: java.lang.IllegalStateException: bad\n\tat com.app.Inner.run(Inner.java:5)";
        let entry = LogEntry::parse(merged).unwrap();
        let d = CrashDetector::new();
        let info = d.parse_crash(&entry, CrashType::FatalException);
        assert_eq!(info.headline, "FATAL EXCEPTION: main");
        assert_eq!(info.exception.as_deref(), Some("java.lang.NullPointerException"));
        assert_eq!(info.stack.len(), 4);
        assert!(info.stack[0].starts_with("at "));
        assert!(info.stack[2].starts_with("Caused by:"));
    }

    #[test]
    fn test_parse_anr_no_stack() {
        let line = "04-02 12:34:56.789  1234  5678 E ActivityManager: ANR in com.example.app (com.example.app/.MainActivity)";
        let entry = LogEntry::parse(line).unwrap();
        let d = CrashDetector::new();
        let info = d.parse_crash(&entry, CrashType::Anr);
        assert!(info.headline.contains("ANR in"));
        assert!(info.stack.is_empty());
        assert!(info.exception.is_none());
    }
}
