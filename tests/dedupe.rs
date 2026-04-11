use aloggrep::dedupe::{Deduper, Normalizer};
use aloggrep::parser::{Level, LogEntry};

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
    let n = Normalizer::new();
    assert_eq!(n.normalize("timeout after 30000ms"), "timeout after <N>ms");
    assert_eq!(n.normalize("timeout after 100ms"), "timeout after <N>ms");
    assert_eq!(n.normalize("cmd:0x9293 done"), "cmd:<hex> done");
}

#[test]
fn test_normalize_uuid() {
    let n = Normalizer::new();
    let msg = "id=550e8400-e29b-41d4-a716-446655440000 ok";
    assert_eq!(n.normalize(msg), "id=<uuid> ok");
}

#[test]
fn test_normalize_trace_id() {
    let n = Normalizer::new();
    let msg = "trace=13bfbc5e52c0410860c4bfb90d2a4c46 req";
    assert_eq!(n.normalize(msg), "trace=<id> req");
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
    let n = Normalizer::new();
    assert_eq!(n.normalize("flag:2 step:15"), "flag:2 step:15");
    assert_eq!(n.normalize("size:208"), "size:<N>");
}
