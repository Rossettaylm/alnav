use std::collections::BTreeMap;
use std::io::Write;

use crate::parser::{Level, LogEntry};

/// Parse an interval string like "10s", "1m", "5m" into seconds.
pub fn parse_interval(s: &str) -> Result<u64, String> {
    let s = s.trim();
    if s.is_empty() {
        return Err("empty interval".to_string());
    }
    let (num_str, unit) = if s.ends_with('s') {
        (&s[..s.len() - 1], 's')
    } else if s.ends_with('m') {
        (&s[..s.len() - 1], 'm')
    } else if s.ends_with('h') {
        (&s[..s.len() - 1], 'h')
    } else {
        // default: treat bare number as seconds
        (s, 's')
    };

    let n: u64 = num_str
        .parse()
        .map_err(|_| format!("invalid interval number: '{num_str}'"))?;
    if n == 0 {
        return Err("interval must be > 0".to_string());
    }
    let secs = match unit {
        's' => n,
        'm' => n * 60,
        'h' => n * 3600,
        _ => unreachable!(),
    };
    Ok(secs)
}

/// Convert HH:MM:SS to total seconds since midnight.
fn hms_to_secs(hms: &str) -> Option<u64> {
    if hms.len() < 8 {
        return None;
    }
    let h: u64 = hms[0..2].parse().ok()?;
    let m: u64 = hms[3..5].parse().ok()?;
    let s: u64 = hms[6..8].parse().ok()?;
    Some(h * 3600 + m * 60 + s)
}

/// Snap seconds-since-midnight down to nearest interval boundary, return "HH:MM:SS".
fn snap_secs(secs: u64, interval: u64) -> String {
    let snapped = (secs / interval) * interval;
    let h = snapped / 3600;
    let m = (snapped % 3600) / 60;
    let s = snapped % 60;
    format!("{h:02}:{m:02}:{s:02}")
}

/// Build the bucket key from a log entry's timestamp.
fn bucket_key(entry: &LogEntry, interval_secs: u64) -> Option<String> {
    // Try full datetime first (xlog: "YYYY-MM-DD HH:MM:SS", threadtime: "MM-DD HH:MM:SS")
    let ts = entry.timestamp.trim();

    if ts.len() >= 19 && ts.as_bytes()[4] == b'-' {
        // xlog: YYYY-MM-DD HH:MM:SS.mmm
        let date_part = &ts[..10]; // "YYYY-MM-DD"
        let hms = &ts[11..19];     // "HH:MM:SS"
        let secs = hms_to_secs(hms)?;
        let snapped = snap_secs(secs, interval_secs);
        Some(format!("{date_part} {snapped}"))
    } else if ts.len() >= 14 {
        // threadtime: MM-DD HH:MM:SS.mmm
        let date_part = &ts[..5];  // "MM-DD"
        let hms = &ts[6..14];      // "HH:MM:SS"
        let secs = hms_to_secs(hms)?;
        let snapped = snap_secs(secs, interval_secs);
        Some(format!("{date_part} {snapped}"))
    } else {
        None
    }
}

#[derive(Default)]
struct Bucket {
    v: usize,
    d: usize,
    i: usize,
    w: usize,
    e: usize,
    f: usize,
}

impl Bucket {
    fn add(&mut self, level: Level) {
        match level {
            Level::V => self.v += 1,
            Level::D => self.d += 1,
            Level::I => self.i += 1,
            Level::W => self.w += 1,
            Level::E => self.e += 1,
            Level::F => self.f += 1,
        }
    }

    fn total(&self) -> usize {
        self.v + self.d + self.i + self.w + self.e + self.f
    }
}

pub struct Histogram {
    interval_secs: u64,
    buckets: BTreeMap<String, Bucket>,
}

impl Histogram {
    pub fn new(interval_secs: u64) -> Self {
        Self {
            interval_secs,
            buckets: BTreeMap::new(),
        }
    }

    pub fn record(&mut self, entry: &LogEntry) {
        if let Some(key) = bucket_key(entry, self.interval_secs) {
            self.buckets.entry(key).or_default().add(entry.level);
        }
    }

    pub fn write_json<W: Write>(&self, out: &mut W) -> std::io::Result<()> {
        write!(out, "[")?;
        for (i, (key, b)) in self.buckets.iter().enumerate() {
            if i > 0 {
                write!(out, ",")?;
            }
            write!(
                out,
                "\n  {{\"bucket\":\"{key}\",\"total\":{},\"V\":{},\"D\":{},\"I\":{},\"W\":{},\"E\":{},\"F\":{}}}",
                b.total(), b.v, b.d, b.i, b.w, b.e, b.f,
            )?;
        }
        writeln!(out, "\n]")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::LogEntry;

    #[test]
    fn test_parse_interval() {
        assert_eq!(parse_interval("10s").unwrap(), 10);
        assert_eq!(parse_interval("1m").unwrap(), 60);
        assert_eq!(parse_interval("5m").unwrap(), 300);
        assert_eq!(parse_interval("1h").unwrap(), 3600);
        assert!(parse_interval("0s").is_err());
        assert!(parse_interval("").is_err());
        assert!(parse_interval("abc").is_err());
    }

    #[test]
    fn test_snap_secs() {
        // 10:32:15 snapped to 10s interval → 10:32:10
        assert_eq!(snap_secs(10 * 3600 + 32 * 60 + 15, 10), "10:32:10");
        // 10:32:15 snapped to 1m interval → 10:32:00
        assert_eq!(snap_secs(10 * 3600 + 32 * 60 + 15, 60), "10:32:00");
        // 10:32:15 snapped to 5m interval → 10:30:00
        assert_eq!(snap_secs(10 * 3600 + 32 * 60 + 15, 300), "10:30:00");
    }

    #[test]
    fn test_histogram_threadtime() {
        let mut h = Histogram::new(60);
        let lines = [
            "04-02 10:32:05.000  1234  5678 E Tag     : err1",
            "04-02 10:32:30.000  1234  5678 W Tag     : warn1",
            "04-02 10:33:05.000  1234  5678 I Tag     : info1",
        ];
        for line in &lines {
            let entry = LogEntry::parse(line).unwrap();
            h.record(&entry);
        }
        assert_eq!(h.buckets.len(), 2);
        let b1 = h.buckets.get("04-02 10:32:00").unwrap();
        assert_eq!(b1.e, 1);
        assert_eq!(b1.w, 1);
        let b2 = h.buckets.get("04-02 10:33:00").unwrap();
        assert_eq!(b2.i, 1);
    }

    #[test]
    fn test_histogram_xlog() {
        let mut h = Histogram::new(10);
        let lines = [
            "2026-03-04 10:32:05.000|1[3542]3831|3542|E|Tag|err",
            "2026-03-04 10:32:12.000|1[3542]3831|3542|E|Tag|err",
            "2026-03-04 10:32:25.000|1[3542]3831|3542|W|Tag|warn",
        ];
        for line in &lines {
            let entry = LogEntry::parse(line).unwrap();
            h.record(&entry);
        }
        assert_eq!(h.buckets.len(), 3);
        assert!(h.buckets.contains_key("2026-03-04 10:32:00"));
        assert!(h.buckets.contains_key("2026-03-04 10:32:10"));
        assert!(h.buckets.contains_key("2026-03-04 10:32:20"));
    }

    #[test]
    fn test_histogram_json_output() {
        let mut h = Histogram::new(60);
        let line = "04-02 10:32:05.000  1234  5678 E Tag     : err";
        h.record(&LogEntry::parse(line).unwrap());

        let mut buf = Vec::new();
        h.write_json(&mut buf).unwrap();
        let json = String::from_utf8(buf).unwrap();
        assert!(json.contains("\"bucket\":\"04-02 10:32:00\""));
        assert!(json.contains("\"E\":1"));
    }
}
