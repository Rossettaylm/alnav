use std::io;

use aloggrep::crash::{CrashDetector, CrashType};
use aloggrep::dedupe::{Deduper, Normalizer};
use aloggrep::expr::Expr;
use aloggrep::filter::FilterChain;
use aloggrep::formatter::{FieldSet, Formatter};
use aloggrep::histogram::{parse_interval, Histogram};
use aloggrep::multiline::MultilineMerger;
use aloggrep::parser::{Level, LogEntry};
use aloggrep::sampler::Sampler;
use aloggrep::summary::Summary;
use aloggrep::{Cli, OutputFormat};

// ═══════════════════════════════════════════════════════════════════════
// Parser edge cases
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn test_parser_threadtime_empty_msg() {
    let line = "04-02 12:34:56.789  1234  5678 D Tag     : ";
    let entry = LogEntry::parse(line).unwrap();
    assert_eq!(entry.msg, "");
    assert_eq!(entry.tag, "Tag");
}

#[test]
fn test_parser_threadtime_special_chars_in_tag() {
    let line = "04-02 12:34:56.789  1234  5678 W com.app.MyService: msg";
    let entry = LogEntry::parse(line).unwrap();
    assert_eq!(entry.tag, "com.app.MyService");
}

#[test]
fn test_parser_threadtime_large_pid_tid() {
    let line = "04-02 12:34:56.789 99999 88888 E Tag     : msg";
    let entry = LogEntry::parse(line).unwrap();
    assert_eq!(entry.pid, "99999");
    assert_eq!(entry.tid, "88888");
}

#[test]
fn test_parser_threadtime_single_digit_pid() {
    let line = "04-02 12:34:56.789  1  2 I Tag     : msg";
    let entry = LogEntry::parse(line).unwrap();
    assert_eq!(entry.pid, "1");
    assert_eq!(entry.tid, "2");
}

#[test]
fn test_parser_brief_no_tid() {
    let line = "E/MyTag(1234): error message";
    let entry = LogEntry::parse(line).unwrap();
    assert_eq!(entry.level, Level::E);
    assert_eq!(entry.tag, "MyTag");
    assert_eq!(entry.pid, "1234");
    assert_eq!(entry.tid, "");
    assert_eq!(entry.timestamp, "");
}

#[test]
fn test_parser_brief_spaces_in_pid() {
    let line = "W/Tag( 123): msg";
    let entry = LogEntry::parse(line).unwrap();
    assert_eq!(entry.pid, "123");
}

#[test]
fn test_parser_brief_all_levels() {
    for (ch, expected) in [('V', Level::V), ('D', Level::D), ('I', Level::I), ('W', Level::W), ('E', Level::E), ('F', Level::F)] {
        let line = format!("{}/Tag(1): msg", ch);
        let entry = LogEntry::parse(&line).unwrap();
        assert_eq!(entry.level, expected);
    }
}

#[test]
fn test_parser_hilog_multi_layer_package() {
    let line = "04-16 11:52:56.297 1234 5678 D A00201/com.tencent.mqq.sub/MyTag: deep package";
    let entry = LogEntry::parse(line).unwrap();
    assert_eq!(entry.pkg, "com.tencent.mqq.sub");
    assert_eq!(entry.tag, "MyTag");
}

#[test]
fn test_parser_hilog_domain_numeric() {
    let line = "04-16 11:52:56.297 1234 5678 I 03C04/SomeTag: msg";
    let entry = LogEntry::parse(line).unwrap();
    assert_eq!(entry.tag, "SomeTag");
    assert_eq!(entry.pkg, "");
}

#[test]
fn test_parser_xlog_empty_msg() {
    let line = "2026-03-04 10:23:28.872|1[3542]3831|3542|W|Tag|";
    let entry = LogEntry::parse(line).unwrap();
    assert_eq!(entry.msg, "");
    assert_eq!(entry.level, Level::W);
}

#[test]
fn test_parser_xlog_msg_with_pipes() {
    let line = "2026-03-04 10:23:28.872|1[3542]3831|3542|I|Tag|msg with | pipes | inside";
    let entry = LogEntry::parse(line).unwrap();
    assert!(entry.msg.contains("msg with | pipes | inside"));
}

#[test]
fn test_parser_xlog_all_levels() {
    for (ch, expected) in [('V', Level::V), ('D', Level::D), ('I', Level::I), ('W', Level::W), ('E', Level::E), ('F', Level::F)] {
        let line = format!("2026-03-04 10:23:28.872|1[3542]3831|3542|{}|Tag|msg", ch);
        let entry = LogEntry::parse(&line).unwrap();
        assert_eq!(entry.level, expected);
    }
}

#[test]
fn test_parser_rejects_incomplete_timestamp() {
    assert!(LogEntry::parse("04-02 12:34").is_none());
    assert!(LogEntry::parse("04-02").is_none());
}

#[test]
fn test_parser_rejects_invalid_level() {
    let line = "04-02 12:34:56.789  1234  5678 X Tag     : msg";
    assert!(LogEntry::parse(line).is_none());
}

#[test]
fn test_parser_level_from_str_variants() {
    assert_eq!(Level::from_str("VERBOSE"), Some(Level::V));
    assert_eq!(Level::from_str("DEBUG"), Some(Level::D));
    assert_eq!(Level::from_str("INFO"), Some(Level::I));
    assert_eq!(Level::from_str("WARN"), Some(Level::W));
    assert_eq!(Level::from_str("WARNING"), Some(Level::W));
    assert_eq!(Level::from_str("ERROR"), Some(Level::E));
    assert_eq!(Level::from_str("FATAL"), Some(Level::F));
    assert_eq!(Level::from_str("v"), Some(Level::V));
    assert_eq!(Level::from_str("unknown"), None);
}

#[test]
fn test_parser_time_hms_brief_returns_none() {
    let line = "E/Tag(1234): msg";
    let entry = LogEntry::parse(line).unwrap();
    assert_eq!(entry.time_hms(), None);
    assert_eq!(entry.time_full(), None);
}

// ═══════════════════════════════════════════════════════════════════════
// Filter advanced tests
// ═══════════════════════════════════════════════════════════════════════

fn make_cli_full(
    tags: &[&str],
    msgs: &[&str],
    level: Option<&str>,
    packages: &[&str],
    and: bool,
    invert: bool,
    ignore_case: bool,
    pid: &[&str],
    tid: &[&str],
    exprs: &[&str],
    since: Option<&str>,
    until: Option<&str>,
) -> Cli {
    Cli {
        tag: tags.iter().map(|s| s.to_string()).collect(),
        msg: msgs.iter().map(|s| s.to_string()).collect(),
        level: level.map(|s| s.to_string()),
        package: packages.iter().map(|s| s.to_string()).collect(),
        file: vec![],
        format: OutputFormat::Text,
        limit: 0,
        count: false,
        summary: false,
        since: since.map(|s| s.to_string()),
        until: until.map(|s| s.to_string()),
        no_color: false,
        ignore_case,
        invert,
        and,
        expr: exprs.iter().map(|s| s.to_string()).collect(),
        context: None,
        after_context: None,
        before_context: None,
        dedupe: false,
        multiline: false,
        crashes: false,
        tail: 0,
        sample: 0,
        pid: pid.iter().map(|s| s.to_string()).collect(),
        tid: tid.iter().map(|s| s.to_string()).collect(),
        histogram: None,
        fields: None,
        sort_time: false,
        time_context: None,
        follow_pid: false,
        follow_tid: false,
        example: false,
        highlight: vec![],
        hdc: false,
        adb: false,
        device: None,
    }
}

#[test]
fn test_filter_ignore_case() {
    let cli = make_cli_full(&["okhttp"], &[], None, &[], false, false, true, &[], &[], &[], None, None);
    let chain = FilterChain::from_cli(&cli).unwrap();
    let line = "04-02 12:34:56.789  1234  5678 D OkHttp  : msg";
    assert!(chain.matches(&LogEntry::parse(line).unwrap()));
}

#[test]
fn test_filter_ignore_case_msg() {
    let cli = make_cli_full(&[], &["ERROR"], None, &[], false, false, true, &[], &[], &[], None, None);
    let chain = FilterChain::from_cli(&cli).unwrap();
    let line = "04-02 12:34:56.789  1234  5678 D Tag     : some error occurred";
    assert!(chain.matches(&LogEntry::parse(line).unwrap()));
}

#[test]
fn test_filter_invert_basic() {
    let cli = make_cli_full(&["OkHttp"], &[], None, &[], false, false, false, &[], &[], &[], None, None);
    let chain = FilterChain::from_cli(&cli).unwrap();
    let line_match = "04-02 12:34:56.789  1234  5678 D OkHttp  : msg";
    let line_no = "04-02 12:34:56.789  1234  5678 D Other   : msg";
    // invert is applied at the call site (main.rs), not in FilterChain::matches
    // so chain.matches returns true for OkHttp, which gets inverted by caller
    assert!(chain.matches(&LogEntry::parse(line_match).unwrap()));
    assert!(!chain.matches(&LogEntry::parse(line_no).unwrap()));
}

#[test]
fn test_filter_package_hilog() {
    let cli = make_cli_full(&[], &[], None, &["com.tencent.mqq"], false, false, false, &[], &[], &[], None, None);
    let chain = FilterChain::from_cli(&cli).unwrap();
    let hit = "04-16 11:52:56.297 11114 11114 I A00201/com.tencent.mqq/QRouter: msg";
    let miss = "04-16 11:52:56.297 11114 11114 I A00201/com.other.app/QRouter: msg";
    assert!(chain.matches(&LogEntry::parse(hit).unwrap()));
    assert!(!chain.matches(&LogEntry::parse(miss).unwrap()));
}

#[test]
fn test_filter_package_fallback_to_tag() {
    // When pkg is empty (threadtime format), package filter checks tag and msg
    let cli = make_cli_full(&[], &[], None, &["OkHttp"], false, false, false, &[], &[], &[], None, None);
    let chain = FilterChain::from_cli(&cli).unwrap();
    let hit = "04-02 12:34:56.789  1234  5678 D OkHttp  : msg";
    let miss = "04-02 12:34:56.789  1234  5678 D Other   : msg";
    assert!(chain.matches(&LogEntry::parse(hit).unwrap()));
    assert!(!chain.matches(&LogEntry::parse(miss).unwrap()));
}

#[test]
fn test_filter_package_fallback_to_msg() {
    let cli = make_cli_full(&[], &[], None, &["com.example"], false, false, false, &[], &[], &[], None, None);
    let chain = FilterChain::from_cli(&cli).unwrap();
    let hit = "04-02 12:34:56.789  1234  5678 D Tag     : crash in com.example.app";
    let miss = "04-02 12:34:56.789  1234  5678 D Tag     : all good";
    assert!(chain.matches(&LogEntry::parse(hit).unwrap()));
    assert!(!chain.matches(&LogEntry::parse(miss).unwrap()));
}

#[test]
fn test_filter_regex_in_tag() {
    let cli = make_cli_full(&["Ok.*|Retro.*"], &[], None, &[], false, false, false, &[], &[], &[], None, None);
    let chain = FilterChain::from_cli(&cli).unwrap();
    let hit1 = "04-02 12:34:56.789  1234  5678 D OkHttp  : msg";
    let hit2 = "04-02 12:34:56.789  1234  5678 D Retrofit: msg";
    let miss = "04-02 12:34:56.789  1234  5678 D Volley  : msg";
    assert!(chain.matches(&LogEntry::parse(hit1).unwrap()));
    assert!(chain.matches(&LogEntry::parse(hit2).unwrap()));
    assert!(!chain.matches(&LogEntry::parse(miss).unwrap()));
}

#[test]
fn test_filter_regex_in_msg() {
    let cli = make_cli_full(&[], &["timeout|connect.*refused"], None, &[], false, false, false, &[], &[], &[], None, None);
    let chain = FilterChain::from_cli(&cli).unwrap();
    let hit1 = "04-02 12:34:56.789  1234  5678 E Tag     : timeout after 30s";
    let hit2 = "04-02 12:34:56.789  1234  5678 E Tag     : connection refused";
    let miss = "04-02 12:34:56.789  1234  5678 E Tag     : success";
    assert!(chain.matches(&LogEntry::parse(hit1).unwrap()));
    assert!(chain.matches(&LogEntry::parse(hit2).unwrap()));
    assert!(!chain.matches(&LogEntry::parse(miss).unwrap()));
}

#[test]
fn test_filter_multiple_tags_or() {
    let cli = make_cli_full(&["OkHttp", "Retrofit"], &[], None, &[], false, false, false, &[], &[], &[], None, None);
    let chain = FilterChain::from_cli(&cli).unwrap();
    let hit1 = "04-02 12:34:56.789  1234  5678 D OkHttp  : msg";
    let hit2 = "04-02 12:34:56.789  1234  5678 D Retrofit: msg";
    let miss = "04-02 12:34:56.789  1234  5678 D Other   : msg";
    assert!(chain.matches(&LogEntry::parse(hit1).unwrap()));
    assert!(chain.matches(&LogEntry::parse(hit2).unwrap()));
    assert!(!chain.matches(&LogEntry::parse(miss).unwrap()));
}

#[test]
fn test_filter_multiple_tags_and() {
    // --tag A --tag B --and means tag must match A AND B
    let cli = make_cli_full(&["Ok", "Http"], &[], None, &[], true, false, false, &[], &[], &[], None, None);
    let chain = FilterChain::from_cli(&cli).unwrap();
    let hit = "04-02 12:34:56.789  1234  5678 D OkHttp  : msg";
    let miss = "04-02 12:34:56.789  1234  5678 D OkApi   : msg";
    assert!(chain.matches(&LogEntry::parse(hit).unwrap()));
    assert!(!chain.matches(&LogEntry::parse(miss).unwrap()));
}

#[test]
fn test_filter_pid_multiple() {
    let cli = make_cli_full(&[], &[], None, &[], false, false, false, &["1234", "5555"], &[], &[], None, None);
    let chain = FilterChain::from_cli(&cli).unwrap();
    let hit1 = "04-02 12:34:56.789  1234  5678 D Tag     : msg";
    let hit2 = "04-02 12:34:56.789  5555  5678 D Tag     : msg";
    let miss = "04-02 12:34:56.789  9999  5678 D Tag     : msg";
    assert!(chain.matches(&LogEntry::parse(hit1).unwrap()));
    assert!(chain.matches(&LogEntry::parse(hit2).unwrap()));
    assert!(!chain.matches(&LogEntry::parse(miss).unwrap()));
}

#[test]
fn test_filter_pid_and_tid_combined() {
    let cli = make_cli_full(&[], &[], None, &[], false, false, false, &["1234"], &["5678"], &[], None, None);
    let chain = FilterChain::from_cli(&cli).unwrap();
    let hit = "04-02 12:34:56.789  1234  5678 D Tag     : msg";
    let miss_pid = "04-02 12:34:56.789  9999  5678 D Tag     : msg";
    let miss_tid = "04-02 12:34:56.789  1234  9999 D Tag     : msg";
    assert!(chain.matches(&LogEntry::parse(hit).unwrap()));
    assert!(!chain.matches(&LogEntry::parse(miss_pid).unwrap()));
    assert!(!chain.matches(&LogEntry::parse(miss_tid).unwrap()));
}

#[test]
fn test_filter_expr_combined_with_tag() {
    let cli = make_cli_full(&["OkHttp"], &[], None, &[], false, false, false, &[], &[], &["level >= W"], None, None);
    let chain = FilterChain::from_cli(&cli).unwrap();
    // Both tag filter and expr must match (AND)
    let hit = "04-02 12:34:56.789  1234  5678 W OkHttp  : msg";
    let miss_level = "04-02 12:34:56.789  1234  5678 D OkHttp  : msg";
    let miss_tag = "04-02 12:34:56.789  1234  5678 W Other   : msg";
    assert!(chain.matches(&LogEntry::parse(hit).unwrap()));
    assert!(!chain.matches(&LogEntry::parse(miss_level).unwrap()));
    assert!(!chain.matches(&LogEntry::parse(miss_tag).unwrap()));
}

#[test]
fn test_filter_multiple_exprs_or() {
    // Multiple -e are OR'd
    let cli = make_cli_full(&[], &[], None, &[], false, false, false, &[], &[], &["tag ~ OkHttp", "tag ~ Retrofit"], None, None);
    let chain = FilterChain::from_cli(&cli).unwrap();
    let hit1 = "04-02 12:34:56.789  1234  5678 D OkHttp  : msg";
    let hit2 = "04-02 12:34:56.789  1234  5678 D Retrofit: msg";
    let miss = "04-02 12:34:56.789  1234  5678 D Other   : msg";
    assert!(chain.matches(&LogEntry::parse(hit1).unwrap()));
    assert!(chain.matches(&LogEntry::parse(hit2).unwrap()));
    assert!(!chain.matches(&LogEntry::parse(miss).unwrap()));
}

#[test]
fn test_filter_is_empty() {
    let cli = make_cli_full(&[], &[], None, &[], false, false, false, &[], &[], &[], None, None);
    let chain = FilterChain::from_cli(&cli).unwrap();
    assert!(chain.is_empty());

    let cli2 = make_cli_full(&["tag"], &[], None, &[], false, false, false, &[], &[], &[], None, None);
    let chain2 = FilterChain::from_cli(&cli2).unwrap();
    assert!(!chain2.is_empty());
}

#[test]
fn test_filter_invalid_regex_returns_error() {
    let cli = make_cli_full(&["[invalid"], &[], None, &[], false, false, false, &[], &[], &[], None, None);
    assert!(FilterChain::from_cli(&cli).is_err());
}

#[test]
fn test_filter_invalid_level_returns_error() {
    let cli = make_cli_full(&[], &[], Some("X"), &[], false, false, false, &[], &[], &[], None, None);
    assert!(FilterChain::from_cli(&cli).is_err());
}

#[test]
fn test_filter_time_since_only() {
    let cli = make_cli_full(&[], &[], None, &[], false, false, false, &[], &[], &[], Some("12:30:00"), None);
    let chain = FilterChain::from_cli(&cli).unwrap();
    let hit = "04-02 12:34:56.789  1234  5678 D Tag     : msg";
    let miss = "04-02 12:00:00.000  1234  5678 D Tag     : msg";
    assert!(chain.matches(&LogEntry::parse(hit).unwrap()));
    assert!(!chain.matches(&LogEntry::parse(miss).unwrap()));
}

#[test]
fn test_filter_time_until_only() {
    let cli = make_cli_full(&[], &[], None, &[], false, false, false, &[], &[], &[], None, Some("12:30:00"));
    let chain = FilterChain::from_cli(&cli).unwrap();
    let hit = "04-02 12:00:00.000  1234  5678 D Tag     : msg";
    let miss = "04-02 12:34:56.789  1234  5678 D Tag     : msg";
    assert!(chain.matches(&LogEntry::parse(hit).unwrap()));
    assert!(!chain.matches(&LogEntry::parse(miss).unwrap()));
}

// ═══════════════════════════════════════════════════════════════════════
// Expr advanced tests
// ═══════════════════════════════════════════════════════════════════════

fn entry_line(level: Level, tag: &str, msg: &str) -> String {
    format!("04-02 12:34:56.789  1234  5678 {} {:<8}: {}", level.as_char(), tag, msg)
}

fn parse_and_eval(expr_str: &str, line: &str) -> bool {
    let expr = Expr::parse(expr_str, false).unwrap();
    let e = LogEntry::parse(line).unwrap();
    expr.matches(&e)
}

#[test]
fn test_expr_pid_match() {
    let line = "04-02 12:34:56.789  1234  5678 D Tag     : msg";
    assert!(parse_and_eval("pid ~ 1234", line));
    assert!(!parse_and_eval("pid ~ 9999", line));
}

#[test]
fn test_expr_tid_match() {
    let line = "04-02 12:34:56.789  1234  5678 D Tag     : msg";
    assert!(parse_and_eval("tid ~ 5678", line));
    assert!(!parse_and_eval("tid ~ 9999", line));
}

#[test]
fn test_expr_pid_regex() {
    let line = "04-02 12:34:56.789  1234  5678 D Tag     : msg";
    assert!(parse_and_eval("pid ~ 12.*", line));
    assert!(!parse_and_eval("pid ~ ^99", line));
}

#[test]
fn test_expr_deeply_nested() {
    let line = entry_line(Level::E, "OkHttp", "timeout error msg");
    let expr_str = "((tag ~ OkHttp or tag ~ Retrofit) and (msg ~ timeout or msg ~ refused)) and level >= W";
    assert!(parse_and_eval(expr_str, &line));

    let line2 = entry_line(Level::D, "OkHttp", "timeout");
    assert!(!parse_and_eval(expr_str, &line2)); // level too low

    let line3 = entry_line(Level::E, "Other", "timeout");
    assert!(!parse_and_eval(expr_str, &line3)); // wrong tag
}

#[test]
fn test_expr_not_with_and() {
    let line = entry_line(Level::E, "OkHttp", "timeout");
    assert!(parse_and_eval("not tag ~ Debug and level >= E", &line));
    assert!(!parse_and_eval("not tag ~ OkHttp and level >= E", &line));
}

#[test]
fn test_expr_multiple_not() {
    let line = entry_line(Level::E, "OkHttp", "err");
    assert!(parse_and_eval("not (not tag ~ OkHttp)", &line));
}

#[test]
fn test_expr_case_insensitive_level() {
    let line = entry_line(Level::W, "T", "m");
    // level value parsed case-insensitively
    let expr = Expr::parse("level >= w", false).unwrap();
    let e = LogEntry::parse(&line).unwrap();
    assert!(expr.matches(&e));
}

#[test]
fn test_expr_quoted_with_spaces() {
    let line = entry_line(Level::I, "App", "hello beautiful world");
    assert!(parse_and_eval("msg ~ \"beautiful world\"", &line));
    assert!(!parse_and_eval("msg ~ \"missing text\"", &line));
}

#[test]
fn test_expr_single_quoted_value() {
    let line = entry_line(Level::I, "App", "hello world");
    assert!(parse_and_eval("msg ~ 'hello world'", &line));
}

#[test]
fn test_expr_error_unknown_field() {
    assert!(Expr::parse("unknown ~ value", false).is_err());
}

#[test]
fn test_expr_error_extra_tokens() {
    assert!(Expr::parse("tag ~ OkHttp extra", false).is_err());
}

#[test]
fn test_expr_error_empty_input() {
    assert!(Expr::parse("", false).is_err());
}

// ═══════════════════════════════════════════════════════════════════════
// Multiline advanced tests
// ═══════════════════════════════════════════════════════════════════════

fn ok_lines(lines: &[&str]) -> Vec<io::Result<String>> {
    lines.iter().map(|s| Ok(s.to_string())).collect()
}

#[test]
fn test_multiline_consecutive_stack_traces() {
    let input = ok_lines(&[
        "04-02 12:34:56.789  1234  5678 E AndroidRuntime: FATAL EXCEPTION: main",
        "    at com.app.Foo.bar(Foo.java:12)",
        "    at com.app.Baz.qux(Baz.java:34)",
        "04-02 12:34:57.000  1234  5678 E AndroidRuntime: FATAL EXCEPTION: worker",
        "    at com.app.Worker.run(Worker.java:5)",
    ]);
    let merged: Vec<String> = MultilineMerger::new(input.into_iter())
        .map(|r| r.unwrap())
        .collect();
    assert_eq!(merged.len(), 2);
    assert!(merged[0].contains("FATAL EXCEPTION: main"));
    assert!(merged[0].contains("at com.app.Foo"));
    assert!(merged[1].contains("FATAL EXCEPTION: worker"));
    assert!(merged[1].contains("at com.app.Worker"));
}

#[test]
fn test_multiline_caused_by_lines() {
    let input = ok_lines(&[
        "04-02 12:34:56.789  1234  5678 E AndroidRuntime: FATAL EXCEPTION: main",
        "java.lang.NullPointerException",
        "    at com.app.Foo.bar(Foo.java:12)",
        "Caused by: java.lang.IllegalStateException",
        "    at com.app.Inner.run(Inner.java:5)",
        "04-02 12:34:58.000  1234  5678 I Tag     : next",
    ]);
    let merged: Vec<String> = MultilineMerger::new(input.into_iter())
        .map(|r| r.unwrap())
        .collect();
    assert_eq!(merged.len(), 2);
    assert!(merged[0].contains("Caused by:"));
    assert!(merged[0].contains("at com.app.Inner"));
}

#[test]
fn test_multiline_empty_input() {
    let input: Vec<io::Result<String>> = vec![];
    let merged: Vec<String> = MultilineMerger::new(input.into_iter())
        .map(|r| r.unwrap())
        .collect();
    assert!(merged.is_empty());
}

#[test]
fn test_multiline_single_entry_no_continuation() {
    let input = ok_lines(&[
        "04-02 12:34:56.789  1234  5678 D Tag     : single line",
    ]);
    let merged: Vec<String> = MultilineMerger::new(input.into_iter())
        .map(|r| r.unwrap())
        .collect();
    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0], "04-02 12:34:56.789  1234  5678 D Tag     : single line");
}

// ═══════════════════════════════════════════════════════════════════════
// Crash advanced tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn test_crash_detect_sigabrt() {
    let line = "04-02 12:34:56.789  1234  5678 F DEBUG   : signal 6 (SIGABRT), code -1";
    let entry = LogEntry::parse(line).unwrap();
    let d = CrashDetector::new();
    assert!(matches!(d.detect(&entry), Some(CrashType::NativeCrash)));
}

#[test]
fn test_crash_detect_sigbus() {
    let line = "04-02 12:34:56.789  1234  5678 F DEBUG   : signal 7 (SIGBUS)";
    let entry = LogEntry::parse(line).unwrap();
    let d = CrashDetector::new();
    assert!(matches!(d.detect(&entry), Some(CrashType::NativeCrash)));
}

#[test]
fn test_crash_detect_sigfpe() {
    let line = "04-02 12:34:56.789  1234  5678 F DEBUG   : signal 8 (SIGFPE)";
    let entry = LogEntry::parse(line).unwrap();
    let d = CrashDetector::new();
    assert!(matches!(d.detect(&entry), Some(CrashType::NativeCrash)));
}

#[test]
fn test_crash_detect_sigill() {
    let line = "04-02 12:34:56.789  1234  5678 F DEBUG   : signal 4 (SIGILL)";
    let entry = LogEntry::parse(line).unwrap();
    let d = CrashDetector::new();
    assert!(matches!(d.detect(&entry), Some(CrashType::NativeCrash)));
}

#[test]
fn test_crash_parse_native() {
    let merged = "04-02 12:34:56.789  1234  5678 F DEBUG   : signal 11 (SIGSEGV), code 1\npid: 1234, tid: 5678\nbacktrace:\n    #00 pc 0x12345 /lib/libc.so";
    let entry = LogEntry::parse(merged).unwrap();
    let d = CrashDetector::new();
    let info = d.parse_crash(&entry, CrashType::NativeCrash);
    assert!(info.headline.contains("SIGSEGV"));
    assert_eq!(info.pid, "1234");
}

#[test]
fn test_crash_detect_case_insensitive() {
    let line = "04-02 12:34:56.789  1234  5678 E AndroidRuntime: fatal exception: main";
    let entry = LogEntry::parse(line).unwrap();
    let d = CrashDetector::new();
    assert!(matches!(d.detect(&entry), Some(CrashType::FatalException)));
}

#[test]
fn test_crash_detect_anr_case_insensitive() {
    let line = "04-02 12:34:56.789  1234  5678 E ActivityManager: anr in com.app";
    let entry = LogEntry::parse(line).unwrap();
    let d = CrashDetector::new();
    assert!(matches!(d.detect(&entry), Some(CrashType::Anr)));
}

// ═══════════════════════════════════════════════════════════════════════
// Dedupe advanced tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn test_dedupe_different_levels_separate_groups() {
    let mut d = Deduper::new();
    let l1 = "04-02 12:34:56.789  1234  5678 E OkHttp  : timeout after 100ms";
    let l2 = "04-02 12:34:57.000  1234  5678 W OkHttp  : timeout after 200ms";
    d.record(&LogEntry::parse(l1).unwrap());
    d.record(&LogEntry::parse(l2).unwrap());
    let groups = d.into_groups();
    // Same pattern but different levels → different groups
    assert_eq!(groups.len(), 2);
}

#[test]
fn test_dedupe_different_tags_separate_groups() {
    let mut d = Deduper::new();
    let l1 = "04-02 12:34:56.789  1234  5678 E OkHttp  : timeout after 100ms";
    let l2 = "04-02 12:34:57.000  1234  5678 E Retrofit: timeout after 200ms";
    d.record(&LogEntry::parse(l1).unwrap());
    d.record(&LogEntry::parse(l2).unwrap());
    let groups = d.into_groups();
    assert_eq!(groups.len(), 2);
}

#[test]
fn test_dedupe_empty_msg() {
    let mut d = Deduper::new();
    let l1 = "04-02 12:34:56.789  1234  5678 E Tag     : ";
    let l2 = "04-02 12:34:57.000  1234  5678 E Tag     : ";
    d.record(&LogEntry::parse(l1).unwrap());
    d.record(&LogEntry::parse(l2).unwrap());
    let groups = d.into_groups();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].count, 2);
}

#[test]
fn test_normalizer_ipv4_not_normalized() {
    let n = Normalizer::new();
    // IP addresses should be partially normalized (digits 3+)
    let result = n.normalize("connect to 192.168.1.100:8080");
    assert!(result.contains("<N>"));
}

#[test]
fn test_normalizer_preserves_short_hex() {
    let n = Normalizer::new();
    assert_eq!(n.normalize("cmd:0xfe done"), "cmd:<hex> done");
}

#[test]
fn test_normalizer_multiple_uuids() {
    let n = Normalizer::new();
    let msg = "a=550e8400-e29b-41d4-a716-446655440000 b=123e4567-e89b-12d3-a456-426614174000";
    let result = n.normalize(msg);
    assert_eq!(result, "a=<uuid> b=<uuid>");
}

// ═══════════════════════════════════════════════════════════════════════
// Sampler edge cases
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn test_sampler_tail_one() {
    let mut s = Sampler::new(1, 0, 0);
    s.should_emit("line1");
    s.should_emit("line2");
    s.should_emit("line3");
    let r = s.finish();
    assert_eq!(r.lines, vec!["line3"]);
}

#[test]
fn test_sampler_empty_input() {
    let s = Sampler::new(10, 0, 0);
    let r = s.finish();
    assert!(r.lines.is_empty());
    assert!(r.header.is_none());
}

#[test]
fn test_sampler_reservoir_empty_input() {
    let s = Sampler::new(0, 10, 0);
    let r = s.finish();
    assert!(r.lines.is_empty());
    assert!(r.header.is_none());
}

#[test]
fn test_sampler_head_tail_exact_fit() {
    // head=2, tail=2, input=4 → emit first 2, buffer last 2, no skip
    let mut s = Sampler::new(2, 0, 2);
    assert!(s.should_emit("a"));
    assert!(s.should_emit("b"));
    assert!(!s.should_emit("c"));
    assert!(!s.should_emit("d"));
    let r = s.finish();
    assert_eq!(r.lines, vec!["c", "d"]);
    assert!(r.header.is_none()); // no skip message when nothing omitted
}

#[test]
fn test_sampler_passthrough_needs_no_full_scan() {
    let s = Sampler::new(0, 0, 0);
    assert!(!s.needs_full_scan());
}

#[test]
fn test_sampler_tail_needs_full_scan() {
    let s = Sampler::new(5, 0, 0);
    assert!(s.needs_full_scan());
}

// ═══════════════════════════════════════════════════════════════════════
// Histogram advanced tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn test_histogram_hour_interval() {
    let mut h = Histogram::new(3600);
    let lines = [
        "04-02 10:32:05.000  1234  5678 E Tag     : err1",
        "04-02 10:45:00.000  1234  5678 W Tag     : warn1",
        "04-02 11:10:00.000  1234  5678 I Tag     : info1",
    ];
    for line in &lines {
        h.record(&LogEntry::parse(line).unwrap());
    }
    let mut buf = Vec::new();
    h.write_json(&mut buf).unwrap();
    let json = String::from_utf8(buf).unwrap();
    assert!(json.contains("\"bucket\":\"04-02 10:00:00\""));
    assert!(json.contains("\"bucket\":\"04-02 11:00:00\""));
}

#[test]
fn test_histogram_brief_format_skipped() {
    let mut h = Histogram::new(60);
    let line = "E/Tag(1234): err msg";
    h.record(&LogEntry::parse(line).unwrap());
    // Brief format has no timestamp → should not create any bucket
    let mut buf = Vec::new();
    h.write_json(&mut buf).unwrap();
    let json = String::from_utf8(buf).unwrap();
    assert!(!json.contains("bucket"));
}

#[test]
fn test_histogram_multiple_levels_same_bucket() {
    let mut h = Histogram::new(60);
    let lines = [
        "04-02 10:32:05.000  1234  5678 V Tag     : verbose",
        "04-02 10:32:10.000  1234  5678 D Tag     : debug",
        "04-02 10:32:15.000  1234  5678 I Tag     : info",
        "04-02 10:32:20.000  1234  5678 W Tag     : warn",
        "04-02 10:32:25.000  1234  5678 E Tag     : error",
        "04-02 10:32:30.000  1234  5678 F Tag     : fatal",
    ];
    for line in &lines {
        h.record(&LogEntry::parse(line).unwrap());
    }
    let mut buf = Vec::new();
    h.write_json(&mut buf).unwrap();
    let json = String::from_utf8(buf).unwrap();
    assert!(json.contains("\"V\":1"));
    assert!(json.contains("\"D\":1"));
    assert!(json.contains("\"I\":1"));
    assert!(json.contains("\"W\":1"));
    assert!(json.contains("\"E\":1"));
    assert!(json.contains("\"F\":1"));
    assert!(json.contains("\"total\":6"));
}

#[test]
fn test_parse_interval_bare_number() {
    // Bare number treated as seconds
    assert_eq!(parse_interval("30").unwrap(), 30);
}

#[test]
fn test_parse_interval_hour() {
    assert_eq!(parse_interval("2h").unwrap(), 7200);
}

// ═══════════════════════════════════════════════════════════════════════
// Formatter advanced tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn test_fieldset_parse_basic() {
    let fs = FieldSet::parse("timestamp,level,msg").unwrap();
    assert!(fs.timestamp);
    assert!(fs.level);
    assert!(fs.msg);
    assert!(!fs.pid);
    assert!(!fs.tid);
    assert!(!fs.tag);
    assert!(!fs.pkg);
}

#[test]
fn test_fieldset_parse_aliases() {
    let fs = FieldSet::parse("ts,lvl,message,package").unwrap();
    assert!(fs.timestamp);
    assert!(fs.level);
    assert!(fs.msg);
    assert!(fs.pkg);
}

#[test]
fn test_fieldset_parse_error_unknown() {
    assert!(FieldSet::parse("bogus").is_err());
}

#[test]
fn test_fieldset_parse_error_empty() {
    assert!(FieldSet::parse("").is_err());
}

#[test]
fn test_formatter_json_entry() {
    let cli = Cli::parse_from(["aloggrep"]);
    let chain = FilterChain::from_cli(&cli).unwrap();
    let f = Formatter::new(OutputFormat::Json, false, &chain, FieldSet::all(), &[]);

    let line = "04-02 12:34:56.789  1234  5678 E OkHttp  : timeout error";
    let entry = LogEntry::parse(line).unwrap();
    let mut buf = Vec::new();
    f.write_entry(&entry, line, &mut buf).unwrap();
    let out = String::from_utf8(buf).unwrap();
    assert!(out.contains("\"timestamp\":\"04-02 12:34:56.789\""));
    assert!(out.contains("\"pid\":\"1234\""));
    assert!(out.contains("\"tid\":\"5678\""));
    assert!(out.contains("\"level\":\"E\""));
    assert!(out.contains("\"tag\":\"OkHttp\""));
    assert!(out.contains("\"msg\":\"timeout error\""));
}

#[test]
fn test_formatter_json_escapes_msg() {
    let cli = Cli::parse_from(["aloggrep"]);
    let chain = FilterChain::from_cli(&cli).unwrap();
    let f = Formatter::new(OutputFormat::Json, false, &chain, FieldSet::all(), &[]);

    let line = "04-02 12:34:56.789  1234  5678 E Tag     : msg with \"quotes\" and \\slash";
    let entry = LogEntry::parse(line).unwrap();
    let mut buf = Vec::new();
    f.write_entry(&entry, line, &mut buf).unwrap();
    let out = String::from_utf8(buf).unwrap();
    assert!(out.contains("\\\"quotes\\\""));
    assert!(out.contains("\\\\slash"));
}

#[test]
fn test_formatter_csv_entry() {
    let cli = Cli::parse_from(["aloggrep"]);
    let chain = FilterChain::from_cli(&cli).unwrap();
    let f = Formatter::new(OutputFormat::Csv, false, &chain, FieldSet::all(), &[]);

    let line = "04-02 12:34:56.789  1234  5678 W OkHttp  : request timeout";
    let entry = LogEntry::parse(line).unwrap();
    let mut buf = Vec::new();
    f.write_entry(&entry, line, &mut buf).unwrap();
    let out = String::from_utf8(buf).unwrap();
    assert!(out.contains("04-02 12:34:56.789"));
    assert!(out.contains("1234"));
    assert!(out.contains("5678"));
    assert!(out.contains("W"));
    assert!(out.contains("OkHttp"));
    assert!(out.contains("\"request timeout\""));
}

#[test]
fn test_formatter_csv_escapes_quotes_in_msg() {
    let cli = Cli::parse_from(["aloggrep"]);
    let chain = FilterChain::from_cli(&cli).unwrap();
    let f = Formatter::new(OutputFormat::Csv, false, &chain, FieldSet::all(), &[]);

    let line = "04-02 12:34:56.789  1234  5678 E Tag     : say \"hello\"";
    let entry = LogEntry::parse(line).unwrap();
    let mut buf = Vec::new();
    f.write_entry(&entry, line, &mut buf).unwrap();
    let out = String::from_utf8(buf).unwrap();
    // CSV escaping: double quotes become ""
    assert!(out.contains("\"\"hello\"\""));
}

#[test]
fn test_formatter_text_no_color_raw() {
    let cli = Cli::parse_from(["aloggrep"]);
    let chain = FilterChain::from_cli(&cli).unwrap();
    let f = Formatter::new(OutputFormat::Text, false, &chain, FieldSet::all(), &[]);

    let line = "04-02 12:34:56.789  1234  5678 D OkHttp  : hello";
    let entry = LogEntry::parse(line).unwrap();
    let mut buf = Vec::new();
    f.write_entry(&entry, line, &mut buf).unwrap();
    let out = String::from_utf8(buf).unwrap();
    assert_eq!(out.trim(), line);
}

#[test]
fn test_formatter_field_selection_json() {
    let cli = Cli::parse_from(["aloggrep"]);
    let chain = FilterChain::from_cli(&cli).unwrap();
    let fields = FieldSet::parse("level,tag,msg").unwrap();
    let f = Formatter::new(OutputFormat::Json, false, &chain, fields, &[]);

    let line = "04-02 12:34:56.789  1234  5678 E OkHttp  : error";
    let entry = LogEntry::parse(line).unwrap();
    let mut buf = Vec::new();
    f.write_entry(&entry, line, &mut buf).unwrap();
    let out = String::from_utf8(buf).unwrap();
    assert!(out.contains("\"level\":\"E\""));
    assert!(out.contains("\"tag\":\"OkHttp\""));
    assert!(out.contains("\"msg\":\"error\""));
    assert!(!out.contains("\"timestamp\""));
    assert!(!out.contains("\"pid\""));
    assert!(!out.contains("\"tid\""));
}

#[test]
fn test_formatter_field_selection_text() {
    let cli = Cli::parse_from(["aloggrep"]);
    let chain = FilterChain::from_cli(&cli).unwrap();
    let fields = FieldSet::parse("tag,msg").unwrap();
    let f = Formatter::new(OutputFormat::Text, false, &chain, fields, &[]);

    let line = "04-02 12:34:56.789  1234  5678 E OkHttp  : error msg";
    let entry = LogEntry::parse(line).unwrap();
    let mut buf = Vec::new();
    f.write_entry(&entry, line, &mut buf).unwrap();
    let out = String::from_utf8(buf).unwrap();
    assert!(out.contains("OkHttp"));
    assert!(out.contains("error msg"));
    assert!(!out.contains("12:34:56"));
    assert!(!out.contains("1234"));
}

use aloggrep::dedupe::DedupGroup;
use clap::Parser;

#[test]
fn test_formatter_dedupe_text() {
    let cli = Cli::parse_from(["aloggrep"]);
    let chain = FilterChain::from_cli(&cli).unwrap();
    let f = Formatter::new(OutputFormat::Text, false, &chain, FieldSet::all(), &[]);

    let group = DedupGroup {
        count: 5,
        level: Level::E,
        tag: "OkHttp".to_string(),
        pattern: "timeout after <N>ms".to_string(),
        sample_msg: "timeout after 100ms".to_string(),
        first_ts: "04-02 10:00:00.000".to_string(),
        last_ts: "04-02 10:05:00.000".to_string(),
    };
    let mut buf = Vec::new();
    f.write_dedupe_group(&group, &mut buf).unwrap();
    let out = String::from_utf8(buf).unwrap();
    assert!(out.contains("5x"));
    assert!(out.contains("OkHttp"));
    assert!(out.contains("timeout after <N>ms"));
}

#[test]
fn test_formatter_dedupe_json() {
    let cli = Cli::parse_from(["aloggrep"]);
    let chain = FilterChain::from_cli(&cli).unwrap();
    let f = Formatter::new(OutputFormat::Json, false, &chain, FieldSet::all(), &[]);

    let group = DedupGroup {
        count: 3,
        level: Level::W,
        tag: "Net".to_string(),
        pattern: "retry <N>".to_string(),
        sample_msg: "retry 5".to_string(),
        first_ts: "04-02 10:00:00.000".to_string(),
        last_ts: "04-02 10:01:00.000".to_string(),
    };
    let mut buf = Vec::new();
    f.write_dedupe_group(&group, &mut buf).unwrap();
    let out = String::from_utf8(buf).unwrap();
    assert!(out.contains("\"count\":3"));
    assert!(out.contains("\"tag\":\"Net\""));
    assert!(out.contains("\"pattern\":\"retry <N>\""));
}

#[test]
fn test_formatter_dedupe_csv() {
    let cli = Cli::parse_from(["aloggrep"]);
    let chain = FilterChain::from_cli(&cli).unwrap();
    let f = Formatter::new(OutputFormat::Csv, false, &chain, FieldSet::all(), &[]);

    let group = DedupGroup {
        count: 2,
        level: Level::I,
        tag: "App".to_string(),
        pattern: "loaded in <N>ms".to_string(),
        sample_msg: "loaded in 250ms".to_string(),
        first_ts: "04-02 10:00:00.000".to_string(),
        last_ts: "04-02 10:02:00.000".to_string(),
    };
    let mut buf = Vec::new();
    f.write_dedupe_group(&group, &mut buf).unwrap();
    let out = String::from_utf8(buf).unwrap();
    assert!(out.contains("2,I,App"));
    assert!(out.contains("loaded in <N>ms"));
}

// ═══════════════════════════════════════════════════════════════════════
// Summary advanced tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn test_summary_time_range() {
    let mut s = Summary::new();
    let l1 = "04-02 10:00:00.000  1234  5678 D Tag     : first";
    let l2 = "04-02 10:05:00.000  1234  5678 D Tag     : last";
    s.record(&LogEntry::parse(l1).unwrap());
    s.record(&LogEntry::parse(l2).unwrap());
    let json = s.to_json(2);
    assert!(json.contains("\"first\":\"04-02 10:00:00.000\""));
    assert!(json.contains("\"last\":\"04-02 10:05:00.000\""));
}

#[test]
fn test_summary_empty() {
    let s = Summary::new();
    let json = s.to_json(0);
    assert!(json.contains("\"total\":0"));
    assert!(json.contains("\"matched\":0"));
    assert!(json.contains("\"crashes\":0"));
    assert!(json.contains("\"top_tags\":[]"));
    assert!(json.contains("\"top_errors\":[]"));
}

#[test]
fn test_summary_multiple_crashes() {
    let mut s = Summary::new();
    let lines = [
        "04-02 10:00:00.000  1234  5678 E AndroidRuntime: FATAL EXCEPTION: main",
        "04-02 10:01:00.000  1234  5678 E ActivityManager: ANR in com.app",
        "04-02 10:02:00.000  1234  5678 F DEBUG   : signal 11 (SIGSEGV)",
    ];
    for line in &lines {
        s.record(&LogEntry::parse(line).unwrap());
    }
    let json = s.to_json(3);
    assert!(json.contains("\"crashes\":3"));
}

#[test]
fn test_summary_level_distribution() {
    let mut s = Summary::new();
    for _ in 0..3 {
        s.record(&LogEntry::parse("04-02 10:00:00.000  1 2 D T: m").unwrap());
    }
    for _ in 0..2 {
        s.record(&LogEntry::parse("04-02 10:00:00.000  1 2 W T: m").unwrap());
    }
    s.record(&LogEntry::parse("04-02 10:00:00.000  1 2 E T: m").unwrap());
    let json = s.to_json(6);
    assert!(json.contains("\"D\":3"));
    assert!(json.contains("\"W\":2"));
    assert!(json.contains("\"E\":1"));
}

#[test]
fn test_summary_top_tags_limited_to_10() {
    let mut s = Summary::new();
    for i in 0..15 {
        let line = format!("04-02 10:00:00.000  1 2 I Tag{:<4}: msg", i);
        s.record(&LogEntry::parse(&line).unwrap());
    }
    let json = s.to_json(15);
    // Should only have 10 top_tags
    let count = json.matches("\"tag\":").count();
    assert!(count <= 10);
}
