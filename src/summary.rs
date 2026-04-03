use std::collections::HashMap;

use serde::Serialize;

use crate::dedupe::Normalizer;
use crate::parser::{Level, LogEntry};

#[derive(Serialize)]
struct SummaryOutput {
    total: usize,
    matched: usize,
    levels: HashMap<char, usize>,
    top_tags: Vec<(String, usize)>,
    time_range: TimeRange,
    top_errors: Vec<ErrorEntry>,
    crashes: usize,
}

#[derive(Serialize)]
struct TimeRange {
    first: String,
    last: String,
}

#[derive(Serialize)]
struct ErrorEntry {
    pattern: String,
    count: usize,
    tag: String,
    sample: String,
}

pub struct Summary {
    levels: HashMap<char, usize>,
    tags: HashMap<String, usize>,
    total: usize,
    first_ts: String,
    last_ts: String,
    error_patterns: HashMap<ErrorKey, ErrorData>,
    crashes: usize,
    normalizer: Normalizer,
}

#[derive(Hash, Eq, PartialEq)]
struct ErrorKey {
    tag: String,
    pattern: String,
}

struct ErrorData {
    count: usize,
    sample: String,
}

impl Summary {
    pub fn new() -> Self {
        Self {
            levels: HashMap::new(),
            tags: HashMap::new(),
            total: 0,
            first_ts: String::new(),
            last_ts: String::new(),
            error_patterns: HashMap::new(),
            crashes: 0,
            normalizer: Normalizer::new(),
        }
    }

    pub fn record(&mut self, entry: &LogEntry) {
        self.total += 1;
        *self.levels.entry(entry.level.as_char()).or_insert(0) += 1;

        if let Some(c) = self.tags.get_mut(entry.tag) {
            *c += 1;
        } else {
            self.tags.insert(entry.tag.to_string(), 1);
        }

        let ts = entry.timestamp.trim();
        if self.first_ts.is_empty() {
            self.first_ts = ts.to_string();
        }
        self.last_ts.clear();
        self.last_ts.push_str(ts);

        // Track error patterns for E/F levels
        if entry.level >= Level::E {
            let first_line = entry.msg.lines().next().unwrap_or(entry.msg);
            let pattern = self.normalizer.normalize(first_line);
            let key = ErrorKey {
                tag: entry.tag.to_string(),
                pattern,
            };
            if let Some(data) = self.error_patterns.get_mut(&key) {
                data.count += 1;
            } else {
                self.error_patterns.insert(key, ErrorData {
                    count: 1,
                    sample: first_line.to_string(),
                });
            }
        }

        // Count crashes
        if is_crash_msg(entry.msg) {
            self.crashes += 1;
        }
    }

    pub fn to_json(self, matched: usize) -> String {
        let mut top_tags: Vec<(String, usize)> = self.tags.into_iter().collect();
        top_tags.sort_by(|a, b| b.1.cmp(&a.1));
        top_tags.truncate(10);

        let mut top_errors: Vec<ErrorEntry> = self.error_patterns
            .into_iter()
            .map(|(key, data)| ErrorEntry {
                pattern: key.pattern,
                count: data.count,
                tag: key.tag,
                sample: data.sample,
            })
            .collect();
        top_errors.sort_by(|a, b| b.count.cmp(&a.count));
        top_errors.truncate(10);

        let output = SummaryOutput {
            total: self.total,
            matched,
            levels: self.levels,
            top_tags,
            time_range: TimeRange {
                first: self.first_ts,
                last: self.last_ts,
            },
            top_errors,
            crashes: self.crashes,
        };

        serde_json::to_string(&output).unwrap_or_else(|_| "{}".to_string())
    }
}

fn is_crash_msg(msg: &str) -> bool {
    msg.contains("FATAL EXCEPTION")
        || msg.contains("ANR in ")
        || msg.contains("SIGSEGV")
        || msg.contains("SIGABRT")
        || msg.contains("SIGBUS")
        || msg.contains("SIGFPE")
        || msg.contains("SIGILL")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::LogEntry;

    #[test]
    fn test_summary_basic() {
        let mut s = Summary::new();
        let l1 = "04-02 10:00:00.000  1234  5678 E OkHttp  : timeout after 100ms";
        let l2 = "04-02 10:01:00.000  1234  5678 W Retrofit: slow response";
        s.record(&LogEntry::parse(l1).unwrap());
        s.record(&LogEntry::parse(l2).unwrap());

        let json = s.to_json(2);
        assert!(json.contains("\"total\":2"));
        assert!(json.contains("\"matched\":2"));
    }

    #[test]
    fn test_summary_top_errors() {
        let mut s = Summary::new();
        for i in 0..5 {
            let line = format!("04-02 10:00:0{i}.000  1234  5678 E OkHttp  : timeout after {i}00ms");
            s.record(&LogEntry::parse(&line).unwrap());
        }
        let line = "04-02 10:01:00.000  1234  5678 E DB      : connection lost";
        s.record(&LogEntry::parse(line).unwrap());

        let json = s.to_json(6);
        // "timeout after <N>ms" grouped as one pattern with count=5
        assert!(json.contains("timeout after <N>ms"));
        assert!(json.contains("\"count\":5"));
        assert!(json.contains("connection lost"));
    }

    #[test]
    fn test_summary_crash_count() {
        let mut s = Summary::new();
        let l1 = "04-02 10:00:00.000  1234  5678 E AndroidRuntime: FATAL EXCEPTION: main";
        let l2 = "04-02 10:01:00.000  1234  5678 E ActivityManager: ANR in com.app";
        let l3 = "04-02 10:02:00.000  1234  5678 W OkHttp  : timeout";
        s.record(&LogEntry::parse(l1).unwrap());
        s.record(&LogEntry::parse(l2).unwrap());
        s.record(&LogEntry::parse(l3).unwrap());

        let json = s.to_json(3);
        assert!(json.contains("\"crashes\":2"));
    }

    #[test]
    fn test_summary_ignores_warnings_in_errors() {
        let mut s = Summary::new();
        let line = "04-02 10:00:00.000  1234  5678 W Tag     : some warning";
        s.record(&LogEntry::parse(line).unwrap());

        let json = s.to_json(1);
        assert!(json.contains("\"top_errors\":[]"));
    }
}
