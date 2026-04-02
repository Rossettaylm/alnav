use std::collections::HashMap;

use serde::Serialize;

use crate::parser::LogEntry;

#[derive(Serialize)]
struct SummaryOutput {
    total: usize,
    matched: usize,
    levels: HashMap<char, usize>,
    top_tags: Vec<(String, usize)>,
    time_range: TimeRange,
}

#[derive(Serialize)]
struct TimeRange {
    first: String,
    last: String,
}

pub struct Summary {
    levels: HashMap<char, usize>,
    tags: HashMap<String, usize>,
    total: usize,
    first_ts: String,
    last_ts: String,
}

impl Summary {
    pub fn new() -> Self {
        Self {
            levels: HashMap::new(),
            tags: HashMap::new(),
            total: 0,
            first_ts: String::new(),
            last_ts: String::new(),
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
    }

    pub fn to_json(self, matched: usize) -> String {
        let mut top_tags: Vec<(String, usize)> = self.tags.into_iter().collect();
        top_tags.sort_by(|a, b| b.1.cmp(&a.1));
        top_tags.truncate(10);

        let output = SummaryOutput {
            total: self.total,
            matched,
            levels: self.levels,
            top_tags,
            time_range: TimeRange {
                first: self.first_ts,
                last: self.last_ts,
            },
        };

        serde_json::to_string(&output).unwrap_or_else(|_| "{}".to_string())
    }
}
