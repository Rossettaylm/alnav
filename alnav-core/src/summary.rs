use std::collections::HashMap;

use serde::Serialize;

use crate::dedupe::Normalizer;
use crate::parser::{Level, LogEntry};

#[derive(Serialize)]
struct SummaryOutput {
    total: usize,
    matched: usize,
    levels: HashMap<char, usize>,
    top_tags: Vec<TagEntry>,
    time_range: TimeRange,
    top_errors: Vec<ErrorEntry>,
    crashes: usize,
}

#[derive(Serialize)]
struct TagEntry {
    tag: String,
    count: usize,
    levels: HashMap<char, usize>,
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
    tags: HashMap<String, TagStats>,
    total: usize,
    first_ts: String,
    last_ts: String,
    error_patterns: HashMap<ErrorKey, ErrorData>,
    crashes: usize,
    normalizer: Normalizer,
}

struct TagStats {
    total: usize,
    levels: HashMap<char, usize>,
}

impl TagStats {
    fn new() -> Self {
        Self {
            total: 0,
            levels: HashMap::new(),
        }
    }
    fn record(&mut self, level_char: char) {
        self.total += 1;
        *self.levels.entry(level_char).or_insert(0) += 1;
    }
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
            c.record(entry.level.as_char());
        } else {
            let mut stats = TagStats::new();
            stats.record(entry.level.as_char());
            self.tags.insert(entry.tag.to_string(), stats);
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
                self.error_patterns.insert(
                    key,
                    ErrorData {
                        count: 1,
                        sample: first_line.to_string(),
                    },
                );
            }
        }

        // Count crashes
        if is_crash_msg(entry.msg) {
            self.crashes += 1;
        }
    }

    pub fn to_json(self, matched: usize) -> String {
        let mut top_tags: Vec<TagEntry> = self
            .tags
            .into_iter()
            .map(|(tag, stats)| TagEntry {
                tag,
                count: stats.total,
                levels: stats.levels,
            })
            .collect();
        top_tags.sort_by(|a, b| b.count.cmp(&a.count));
        top_tags.truncate(10);

        let mut top_errors: Vec<ErrorEntry> = self
            .error_patterns
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
