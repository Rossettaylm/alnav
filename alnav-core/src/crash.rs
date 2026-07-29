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
            // Require `signal <n> (` (tombstone form) — bare `signal 4` in
            // ordinary prose (e.g. "connect signal 4") must not match.
            native_re: Regex::new(
                r"(?i)(SIGSEGV|SIGABRT|SIGBUS|SIGFPE|SIGILL|signal \d+\s*\()",
            )
            .unwrap(),
            exception_re: Regex::new(
                r"(?:Caused by: |\n)([a-zA-Z][\w.]*(?:Exception|Error|Throwable))",
            )
            .unwrap(),
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

        let exception = self
            .exception_re
            .captures(msg)
            .map(|caps| caps.get(1).unwrap().as_str().to_string());

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
