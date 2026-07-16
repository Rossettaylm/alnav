use aloggrep::parser::{Level, LogEntry};

#[test]
fn test_threadtime() {
    let line = "04-02 12:34:56.789  1234  5678 W OkHttp  : Connection timeout after 30s";
    let entry = LogEntry::parse(line).unwrap();
    assert_eq!(entry.timestamp, "04-02 12:34:56.789");
    assert_eq!(entry.pid, "1234");
    assert_eq!(entry.tid, "5678");
    assert_eq!(entry.level, Level::W);
    assert_eq!(entry.tag, "OkHttp");
    assert_eq!(entry.msg, "Connection timeout after 30s");
}

#[test]
fn test_brief() {
    let line = "W/OkHttp(1234): Connection timeout";
    let entry = LogEntry::parse(line).unwrap();
    assert_eq!(entry.level, Level::W);
    assert_eq!(entry.tag, "OkHttp");
    assert_eq!(entry.pid, "1234");
    assert_eq!(entry.msg, "Connection timeout");
}

#[test]
fn test_xlog() {
    let line = "2026-03-04 10:23:28.872|1[3542]3831|3542|I|NTKernel|[I] mobile_msf_depend_proxy.cc(101)::SendMsfRequest cmd:test";
    let entry = LogEntry::parse(line).unwrap();
    assert_eq!(entry.timestamp, "2026-03-04 10:23:28.872");
    assert_eq!(entry.pid, "3542");
    assert_eq!(entry.tid, "3542");
    assert_eq!(entry.level, Level::I);
    assert_eq!(entry.tag, "NTKernel");
    assert!(entry.msg.contains("mobile_msf_depend_proxy"));
}

#[test]
fn test_xlog_time_hms() {
    let line = "2026-03-04 10:23:28.872|1[3542]3831|3542|I|Tag|msg";
    let entry = LogEntry::parse(line).unwrap();
    assert_eq!(entry.time_hms(), Some("10:23:28"));
}

#[test]
fn test_hilog() {
    let line = "04-16 11:52:56.297 11114 11114 I A00201/com.tencent.mqq/QRouter: qlog，registerBusinessPageBuilder";
    let entry = LogEntry::parse(line).unwrap();
    assert_eq!(entry.timestamp, "04-16 11:52:56.297");
    assert_eq!(entry.pid, "11114");
    assert_eq!(entry.tid, "11114");
    assert_eq!(entry.level, Level::I);
    assert_eq!(entry.tag, "QRouter");
    assert_eq!(entry.pkg, "com.tencent.mqq");
    assert!(entry.msg.contains("registerBusinessPageBuilder"));
}

#[test]
fn test_hilog_domain_only() {
    // DOMAIN/TAG without package
    let line = "04-16 11:52:56.297 1234 5678 W A00201/SomeTag: warning message";
    let entry = LogEntry::parse(line).unwrap();
    assert_eq!(entry.tag, "SomeTag");
    assert_eq!(entry.pkg, "");
    assert_eq!(entry.level, Level::W);
}

#[test]
fn test_hilog_time_hms() {
    let line = "04-16 11:52:56.297 11114 11114 I A00201/com.tencent.mqq/QRouter: msg";
    let entry = LogEntry::parse(line).unwrap();
    assert_eq!(entry.time_hms(), Some("11:52:56"));
}

#[test]
fn test_hilog_time_full() {
    let line = "04-16 11:52:56.297 11114 11114 I A00201/com.tencent.mqq/QRouter: msg";
    let entry = LogEntry::parse(line).unwrap();
    assert_eq!(entry.time_full(), Some("04-16 11:52:56"));
}

#[test]
fn test_threadtime_no_pkg() {
    let line = "04-02 12:34:56.789  1234  5678 W OkHttp  : Connection timeout after 30s";
    let entry = LogEntry::parse(line).unwrap();
    assert_eq!(entry.pkg, "");
    assert_eq!(entry.tag, "OkHttp");
}

#[test]
fn test_unparseable() {
    assert!(LogEntry::parse("just some random text").is_none());
    assert!(LogEntry::parse("").is_none());
}

#[test]
fn test_level_ordering() {
    assert!(Level::V < Level::D);
    assert!(Level::D < Level::I);
    assert!(Level::W < Level::E);
    assert!(Level::E < Level::F);
}

#[test]
fn test_time_hms() {
    let line = "04-02 12:34:56.789  1234  5678 D Tag     : msg";
    let entry = LogEntry::parse(line).unwrap();
    assert_eq!(entry.time_hms(), Some("12:34:56"));
}

#[test]
fn test_time_full_threadtime() {
    let line = "04-02 12:34:56.789  1234  5678 D Tag     : msg";
    let entry = LogEntry::parse(line).unwrap();
    assert_eq!(entry.time_full(), Some("04-02 12:34:56"));
}

#[test]
fn test_time_full_xlog() {
    let line = "2026-03-04 10:23:28.872|1[3542]3831|3542|I|Tag|msg";
    let entry = LogEntry::parse(line).unwrap();
    assert_eq!(entry.time_full(), Some("2026-03-04 10:23:28"));
}
