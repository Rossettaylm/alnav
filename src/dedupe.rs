use std::collections::HashMap;

use regex::Regex;
use serde::Serialize;

use crate::parser::{Level, LogEntry};

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
    normalizers: Vec<Regex>,
}

#[derive(Hash, Eq, PartialEq, Clone)]
struct DedupKey {
    level: Level,
    tag: String,
    pattern: String,
}

impl Deduper {
    pub fn new() -> Self {
        // Pre-compile normalization regexes (order matters: specific before general)
        let normalizers = vec![
            // UUID: 8-4-4-4-12 hex
            Regex::new(r"[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}").unwrap(),
            // Trace ID: 24+ char hex (OpenTelemetry style)
            Regex::new(r"[0-9a-fA-F]{24,}").unwrap(),
            // Hex number: 0x...
            Regex::new(r"0x[0-9a-fA-F]+").unwrap(),
            // Decimal number: 3+ consecutive digits
            Regex::new(r"[0-9]{3,}").unwrap(),
        ];
        Self {
            groups: HashMap::new(),
            order: Vec::new(),
            normalizers,
        }
    }

    fn normalize(&self, msg: &str) -> String {
        let mut result = msg.to_string();
        // Replace UUIDs
        result = self.normalizers[0].replace_all(&result, "<uuid>").into_owned();
        // Replace trace IDs
        result = self.normalizers[1].replace_all(&result, "<id>").into_owned();
        // Replace hex numbers
        result = self.normalizers[2].replace_all(&result, "<hex>").into_owned();
        // Replace long decimal numbers
        result = self.normalizers[3].replace_all(&result, "<N>").into_owned();
        result
    }

    pub fn record(&mut self, entry: &LogEntry) {
        let pattern = self.normalize(entry.msg);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::LogEntry;

    fn entry_line(level: Level, tag: &str, msg: &str) -> String {
        format!(
            "04-02 12:34:56.789  1234  5678 {} {:<8}: {}",
            level.as_char(),
            tag,
            msg
        )
    }

    #[test]
    fn test_normalize_numbers() {
        let d = Deduper::new();
        assert_eq!(d.normalize("timeout after 30000ms"), "timeout after <N>ms");
        assert_eq!(d.normalize("timeout after 100ms"), "timeout after <N>ms");
        assert_eq!(d.normalize("cmd:0x9293 done"), "cmd:<hex> done");
    }

    #[test]
    fn test_normalize_uuid() {
        let d = Deduper::new();
        let msg = "id=550e8400-e29b-41d4-a716-446655440000 ok";
        assert_eq!(d.normalize(msg), "id=<uuid> ok");
    }

    #[test]
    fn test_normalize_trace_id() {
        let d = Deduper::new();
        let msg = "trace=13bfbc5e52c0410860c4bfb90d2a4c46 req";
        assert_eq!(d.normalize(msg), "trace=<id> req");
    }

    #[test]
    fn test_dedup_grouping() {
        let mut d = Deduper::new();
        let l1 = entry_line(Level::E, "OkHttp", "timeout after 100ms");
        let l2 = entry_line(Level::E, "OkHttp", "timeout after 200ms");
        let l3 = entry_line(Level::E, "OkHttp", "connection refused");

        d.record(&LogEntry::parse(&l1).unwrap());
        d.record(&LogEntry::parse(&l2).unwrap());
        d.record(&LogEntry::parse(&l3).unwrap());

        let groups = d.into_groups();
        // timeout 100ms and 200ms should group (both normalize to "timeout after <N>ms")
        assert_eq!(groups.len(), 2, "groups: {:?}", groups.iter().map(|g| &g.pattern).collect::<Vec<_>>());
        assert_eq!(groups[0].count, 2);
        assert_eq!(groups[0].pattern, "timeout after <N>ms");
        assert_eq!(groups[1].count, 1);
    }

    #[test]
    fn test_dedup_preserves_timestamps() {
        let mut d = Deduper::new();
        let l1 = "04-02 10:00:00.000  1234  5678 E Tag     : err 100";
        let l2 = "04-02 10:05:00.000  1234  5678 E Tag     : err 200";

        d.record(&LogEntry::parse(l1).unwrap());
        d.record(&LogEntry::parse(l2).unwrap());

        let groups = d.into_groups();
        assert_eq!(groups[0].first_ts, "04-02 10:00:00.000");
        assert_eq!(groups[0].last_ts, "04-02 10:05:00.000");
    }

    #[test]
    fn test_short_numbers_not_normalized() {
        let d = Deduper::new();
        // 2-digit numbers should NOT be replaced
        assert_eq!(d.normalize("flag:2 step:15"), "flag:2 step:15");
        // 3+ digits should be replaced
        assert_eq!(d.normalize("size:208"), "size:<N>");
    }
}
