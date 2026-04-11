use aloggrep::expr::Expr;
use aloggrep::parser::{Level, LogEntry};

fn entry(level: Level, tag: &str, msg: &str) -> String {
    format!(
        "04-02 12:34:56.789  1234  5678 {} {:<8}: {}",
        level.as_char(),
        tag,
        msg
    )
}

fn parse_and_match(expr_str: &str, line: &str) -> bool {
    let expr = Expr::parse(expr_str, false).unwrap();
    let e = LogEntry::parse(line).unwrap();
    expr.matches(&e)
}

#[test]
fn test_tag_match() {
    let line = entry(Level::D, "OkHttp", "request ok");
    assert!(parse_and_match("tag ~ OkHttp", &line));
    assert!(!parse_and_match("tag ~ Retrofit", &line));
}

#[test]
fn test_msg_match() {
    let line = entry(Level::I, "App", "timeout error occurred");
    assert!(parse_and_match("msg ~ timeout", &line));
    assert!(!parse_and_match("msg ~ success", &line));
}

#[test]
fn test_pkg_match() {
    let line = entry(Level::I, "com.app", "starting service");
    assert!(parse_and_match("pkg ~ com.app", &line));
    assert!(parse_and_match("pkg ~ service", &line));
    assert!(!parse_and_match("pkg ~ missing", &line));
}

#[test]
fn test_level_gte() {
    let warn = entry(Level::W, "T", "m");
    let debug = entry(Level::D, "T", "m");
    assert!(parse_and_match("level >= W", &warn));
    assert!(!parse_and_match("level >= W", &debug));
}

#[test]
fn test_and() {
    let line = entry(Level::W, "OkHttp", "timeout error");
    assert!(parse_and_match("tag ~ OkHttp and msg ~ timeout", &line));
    assert!(!parse_and_match("tag ~ OkHttp and msg ~ success", &line));
}

#[test]
fn test_or() {
    let line = entry(Level::D, "Retrofit", "ok");
    assert!(parse_and_match("tag ~ OkHttp or tag ~ Retrofit", &line));
    assert!(!parse_and_match("tag ~ OkHttp or tag ~ Volley", &line));
}

#[test]
fn test_not() {
    let line = entry(Level::D, "Debug", "trace");
    assert!(parse_and_match("not tag ~ OkHttp", &line));
    assert!(!parse_and_match("not tag ~ Debug", &line));
}

#[test]
fn test_parens_and_precedence() {
    let ok_warn = entry(Level::W, "OkHttp", "m");
    let ok_debug = entry(Level::D, "OkHttp", "m");
    let other_warn = entry(Level::W, "Other", "m");

    let e = "(tag ~ OkHttp or tag ~ MyApp) and level >= W";
    assert!(parse_and_match(e, &ok_warn));
    assert!(!parse_and_match(e, &ok_debug));
    assert!(!parse_and_match(e, &other_warn));
}

#[test]
fn test_case_insensitive() {
    let line = entry(Level::D, "OkHttp", "hello");
    let expr = Expr::parse("tag ~ okhttp", true).unwrap();
    let e = LogEntry::parse(&line).unwrap();
    assert!(expr.matches(&e));
}

#[test]
fn test_quoted_value() {
    let line = entry(Level::I, "App", "hello world");
    assert!(parse_and_match("msg ~ \"hello world\"", &line));
}

#[test]
fn test_collect_patterns() {
    let expr = Expr::parse("tag ~ OkHttp and msg ~ timeout", false).unwrap();
    let mut pats = Vec::new();
    expr.collect_patterns(&mut pats);
    assert_eq!(pats.len(), 2);
}

#[test]
fn test_error_unterminated_string() {
    assert!(Expr::parse("msg ~ \"hello", false).is_err());
}

#[test]
fn test_error_unexpected_eof() {
    assert!(Expr::parse("tag ~", false).is_err());
}

#[test]
fn test_error_bad_level() {
    assert!(Expr::parse("level >= X", false).is_err());
}

#[test]
fn test_error_missing_paren() {
    assert!(Expr::parse("(tag ~ OkHttp", false).is_err());
}

#[test]
fn test_complex_expr() {
    let line = entry(Level::E, "OkHttp", "mobile_msf cmd:0x9293 done");
    let e = "msg ~ mobile_msf and msg ~ 0x9293 and level >= W";
    assert!(parse_and_match(e, &line));
}
