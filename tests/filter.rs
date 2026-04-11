use aloggrep::filter::FilterChain;
use aloggrep::parser::{Level, LogEntry};
use aloggrep::{Cli, OutputFormat};

fn make_entry(level: Level, tag: &str, msg: &str) -> String {
    format!("04-02 12:34:56.789  1234  5678 {} {:<8}: {}", level.as_char(), tag, msg)
}

fn build_chain_with(
    tags: &[&str],
    msgs: &[&str],
    level: Option<&str>,
    packages: &[&str],
    and: bool,
) -> FilterChain {
    let cli = Cli {
        tag: tags.iter().map(|s| s.to_string()).collect(),
        msg: msgs.iter().map(|s| s.to_string()).collect(),
        level: level.map(|s| s.to_string()),
        package: packages.iter().map(|s| s.to_string()).collect(),
        file: vec![],
        format: OutputFormat::Text,
        limit: 0,
        count: false,
        summary: false,
        since: None,
        until: None,
        no_color: false,
        ignore_case: false,
        invert: false,
        and,
        expr: vec![],
        context: None,
        after_context: None,
        before_context: None,
        dedupe: false,
        multiline: false,
        crashes: false,
        tail: 0,
        sample: 0,
        pid: vec![],
        tid: vec![],
        histogram: None,
        fields: None,
        sort_time: false,
        time_context: None,
        follow_pid: false,
        follow_tid: false,
    };
    FilterChain::from_cli(&cli).unwrap()
}

fn build_chain(tags: &[&str], msgs: &[&str], level: Option<&str>, packages: &[&str]) -> FilterChain {
    build_chain_with(tags, msgs, level, packages, false)
}

#[test]
fn test_empty_chain_matches_all() {
    let chain = build_chain(&[], &[], None, &[]);
    let line = make_entry(Level::D, "Tag", "hello");
    assert!(chain.matches(&LogEntry::parse(&line).unwrap()));
}

#[test]
fn test_tag_filter() {
    let chain = build_chain(&["OkHttp"], &[], None, &[]);
    let hit = make_entry(Level::D, "OkHttp", "request");
    let miss = make_entry(Level::D, "MyApp", "request");
    assert!(chain.matches(&LogEntry::parse(&hit).unwrap()));
    assert!(!chain.matches(&LogEntry::parse(&miss).unwrap()));
}

#[test]
fn test_tag_or_logic() {
    let chain = build_chain(&["OkHttp|Retrofit"], &[], None, &[]);
    assert!(chain.matches(&LogEntry::parse(&make_entry(Level::D, "OkHttp", "m")).unwrap()));
    assert!(chain.matches(&LogEntry::parse(&make_entry(Level::D, "Retrofit", "m")).unwrap()));
    assert!(!chain.matches(&LogEntry::parse(&make_entry(Level::D, "Other", "m")).unwrap()));
}

#[test]
fn test_cross_type_and_logic() {
    let chain = build_chain(&["OkHttp"], &["error"], None, &[]);
    assert!(chain.matches(&LogEntry::parse(&make_entry(Level::D, "OkHttp", "error occurred")).unwrap()));
    assert!(!chain.matches(&LogEntry::parse(&make_entry(Level::D, "OkHttp", "success")).unwrap()));
    assert!(!chain.matches(&LogEntry::parse(&make_entry(Level::D, "MyApp", "error occurred")).unwrap()));
}

#[test]
fn test_level_filter() {
    let chain = build_chain(&[], &[], Some("W"), &[]);
    assert!(chain.matches(&LogEntry::parse(&make_entry(Level::W, "T", "m")).unwrap()));
    assert!(chain.matches(&LogEntry::parse(&make_entry(Level::E, "T", "m")).unwrap()));
    assert!(!chain.matches(&LogEntry::parse(&make_entry(Level::D, "T", "m")).unwrap()));
}

#[test]
fn test_msg_and_logic() {
    let chain = build_chain_with(&[], &["mobile_msf", "0x9293"], None, &[], true);
    assert!(chain.matches(&LogEntry::parse(&make_entry(Level::I, "NT", "mobile_msf cmd:0x9293 done")).unwrap()));
    assert!(!chain.matches(&LogEntry::parse(&make_entry(Level::I, "NT", "mobile_msf cmd:0xfe1 done")).unwrap()));
    assert!(!chain.matches(&LogEntry::parse(&make_entry(Level::I, "NT", "other cmd:0x9293 done")).unwrap()));
}

fn build_chain_time(since: Option<&str>, until: Option<&str>) -> FilterChain {
    let cli = Cli {
        tag: vec![], msg: vec![], level: None, package: vec![],
        file: vec![], format: OutputFormat::Text, limit: 0, count: false,
        summary: false, since: since.map(|s| s.to_string()),
        until: until.map(|s| s.to_string()), no_color: false,
        ignore_case: false, invert: false, and: false, expr: vec![],
        context: None, after_context: None, before_context: None,
        dedupe: false, multiline: false, crashes: false, tail: 0, sample: 0,
        pid: vec![], tid: vec![], histogram: None, fields: None,
        sort_time: false, time_context: None,
        follow_pid: false, follow_tid: false,
    };
    FilterChain::from_cli(&cli).unwrap()
}

#[test]
fn test_time_hms_since_until() {
    let chain = build_chain_time(Some("12:00:00"), Some("13:00:00"));
    let hit = "04-02 12:34:56.789  1234  5678 D Tag     : msg";
    let before = "04-02 11:59:59.999  1234  5678 D Tag     : msg";
    let after = "04-02 13:00:01.000  1234  5678 D Tag     : msg";
    assert!(chain.matches(&LogEntry::parse(hit).unwrap()));
    assert!(!chain.matches(&LogEntry::parse(before).unwrap()));
    assert!(!chain.matches(&LogEntry::parse(after).unwrap()));
}

#[test]
fn test_time_full_datetime_xlog() {
    let chain = build_chain_time(
        Some("2026-03-04 10:30:00"),
        Some("2026-03-04 10:35:00"),
    );
    let hit = "2026-03-04 10:32:00.000|1[3542]3831|3542|I|Tag|msg";
    let before = "2026-03-04 10:29:59.000|1[3542]3831|3542|I|Tag|msg";
    let after = "2026-03-04 10:35:01.000|1[3542]3831|3542|I|Tag|msg";
    assert!(chain.matches(&LogEntry::parse(hit).unwrap()));
    assert!(!chain.matches(&LogEntry::parse(before).unwrap()));
    assert!(!chain.matches(&LogEntry::parse(after).unwrap()));
}

#[test]
fn test_time_full_date_threadtime() {
    let chain = build_chain_time(
        Some("04-02 12:00:00"),
        Some("04-02 13:00:00"),
    );
    let hit = "04-02 12:34:56.789  1234  5678 D Tag     : msg";
    let before = "04-01 23:59:59.999  1234  5678 D Tag     : msg";
    let after = "04-03 00:00:01.000  1234  5678 D Tag     : msg";
    assert!(chain.matches(&LogEntry::parse(hit).unwrap()));
    assert!(!chain.matches(&LogEntry::parse(before).unwrap()));
    assert!(!chain.matches(&LogEntry::parse(after).unwrap()));
}

#[test]
fn test_time_full_xlog_cross_day() {
    let chain = build_chain_time(
        Some("2026-03-03 23:00:00"),
        Some("2026-03-04 01:00:00"),
    );
    let hit = "2026-03-03 23:30:00.000|1[3542]3831|3542|I|Tag|msg";
    let miss = "2026-03-04 02:00:00.000|1[3542]3831|3542|I|Tag|msg";
    assert!(chain.matches(&LogEntry::parse(hit).unwrap()));
    assert!(!chain.matches(&LogEntry::parse(miss).unwrap()));
}

#[test]
fn test_pid_filter() {
    let cli = Cli {
        tag: vec![], msg: vec![], level: None, package: vec![],
        file: vec![], format: OutputFormat::Text, limit: 0, count: false,
        summary: false, since: None, until: None, no_color: false,
        ignore_case: false, invert: false, and: false, expr: vec![],
        context: None, after_context: None, before_context: None,
        dedupe: false, multiline: false, crashes: false, tail: 0, sample: 0,
        pid: vec!["1234".to_string()], tid: vec![],
        histogram: None, fields: None, sort_time: false, time_context: None,
        follow_pid: false, follow_tid: false,
    };
    let chain = FilterChain::from_cli(&cli).unwrap();
    let hit = "04-02 12:34:56.789  1234  5678 D Tag     : msg";
    let miss = "04-02 12:34:56.789  9999  5678 D Tag     : msg";
    assert!(chain.matches(&LogEntry::parse(hit).unwrap()));
    assert!(!chain.matches(&LogEntry::parse(miss).unwrap()));
}

#[test]
fn test_tid_filter() {
    let cli = Cli {
        tag: vec![], msg: vec![], level: None, package: vec![],
        file: vec![], format: OutputFormat::Text, limit: 0, count: false,
        summary: false, since: None, until: None, no_color: false,
        ignore_case: false, invert: false, and: false, expr: vec![],
        context: None, after_context: None, before_context: None,
        dedupe: false, multiline: false, crashes: false, tail: 0, sample: 0,
        pid: vec![], tid: vec!["5678".to_string()],
        histogram: None, fields: None, sort_time: false, time_context: None,
        follow_pid: false, follow_tid: false,
    };
    let chain = FilterChain::from_cli(&cli).unwrap();
    let hit = "04-02 12:34:56.789  1234  5678 D Tag     : msg";
    let miss = "04-02 12:34:56.789  1234  9999 D Tag     : msg";
    assert!(chain.matches(&LogEntry::parse(hit).unwrap()));
    assert!(!chain.matches(&LogEntry::parse(miss).unwrap()));
}
