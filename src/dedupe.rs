use std::collections::HashMap;

use regex::Regex;
use serde::Serialize;

use crate::parser::{Level, LogEntry};

/// Reusable message normalizer: replaces UUIDs, hex, trace IDs, numbers with placeholders.
pub struct Normalizer {
    regexes: Vec<Regex>,
}

impl Normalizer {
    pub fn new() -> Self {
        Self {
            regexes: vec![
                // UUID: 8-4-4-4-12 hex
                Regex::new(r"[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}").unwrap(),
                // Trace ID: 24+ char hex (OpenTelemetry style)
                Regex::new(r"[0-9a-fA-F]{24,}").unwrap(),
                // Hex number: 0x...
                Regex::new(r"0x[0-9a-fA-F]+").unwrap(),
                // Decimal number: 3+ consecutive digits
                Regex::new(r"[0-9]{3,}").unwrap(),
            ],
        }
    }

    pub fn normalize(&self, msg: &str) -> String {
        let mut result = msg.to_string();
        result = self.regexes[0].replace_all(&result, "<uuid>").into_owned();
        result = self.regexes[1].replace_all(&result, "<id>").into_owned();
        result = self.regexes[2].replace_all(&result, "<hex>").into_owned();
        result = self.regexes[3].replace_all(&result, "<N>").into_owned();
        result
    }
}

/// A group of deduplicated log entries sharing the same (level, tag, pattern).
#[derive(Serialize)]
pub struct DedupGroup {
    pub count: usize,
    pub level: Level,
    pub tag: String,
    pub pattern: String,
    pub sample_msg: String,
    pub first_ts: String,
    pub last_ts: String,
}

pub struct Deduper {
    groups: HashMap<DedupKey, DedupGroup>,
    order: Vec<DedupKey>,
    normalizer: Normalizer,
}

#[derive(Hash, Eq, PartialEq, Clone)]
struct DedupKey {
    level: Level,
    tag: String,
    pattern: String,
}

impl Deduper {
    pub fn new() -> Self {
        Self {
            groups: HashMap::new(),
            order: Vec::new(),
            normalizer: Normalizer::new(),
        }
    }

    pub fn record(&mut self, entry: &LogEntry) {
        let pattern = self.normalizer.normalize(entry.msg);
        let key = DedupKey {
            level: entry.level,
            tag: entry.tag.to_string(),
            pattern,
        };

        if let Some(group) = self.groups.get_mut(&key) {
            group.count += 1;
            let ts = entry.timestamp.trim();
            if !ts.is_empty() {
                group.last_ts.clear();
                group.last_ts.push_str(ts);
            }
        } else {
            let ts = entry.timestamp.trim().to_string();
            let group = DedupGroup {
                count: 1,
                level: entry.level,
                tag: entry.tag.to_string(),
                pattern: key.pattern.clone(),
                sample_msg: entry.msg.to_string(),
                first_ts: ts.clone(),
                last_ts: ts,
            };
            self.order.push(key.clone());
            self.groups.insert(key, group);
        }
    }

    /// Consume and return groups sorted by count descending.
    pub fn into_groups(self) -> Vec<DedupGroup> {
        let mut groups: Vec<DedupGroup> = self.groups.into_values().collect();
        groups.sort_by(|a, b| b.count.cmp(&a.count));
        groups
    }
}
