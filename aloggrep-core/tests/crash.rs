use aloggrep::crash::{CrashDetector, CrashType};
use aloggrep::parser::LogEntry;

#[test]
fn test_detect_fatal_exception() {
    let line = "04-02 12:34:56.789  1234  5678 E AndroidRuntime: FATAL EXCEPTION: main";
    let entry = LogEntry::parse(line).unwrap();
    let d = CrashDetector::new();
    assert!(matches!(d.detect(&entry), Some(CrashType::FatalException)));
}

#[test]
fn test_detect_anr() {
    let line = "04-02 12:34:56.789  1234  5678 E ActivityManager: ANR in com.example.app";
    let entry = LogEntry::parse(line).unwrap();
    let d = CrashDetector::new();
    assert!(matches!(d.detect(&entry), Some(CrashType::Anr)));
}

#[test]
fn test_detect_native() {
    let line = "04-02 12:34:56.789  1234  5678 F DEBUG   : signal 11 (SIGSEGV), code 1";
    let entry = LogEntry::parse(line).unwrap();
    let d = CrashDetector::new();
    assert!(matches!(d.detect(&entry), Some(CrashType::NativeCrash)));
}

#[test]
fn test_no_crash() {
    let line = "04-02 12:34:56.789  1234  5678 W OkHttp  : timeout";
    let entry = LogEntry::parse(line).unwrap();
    let d = CrashDetector::new();
    assert!(d.detect(&entry).is_none());
}

#[test]
fn test_parse_crash_with_stack() {
    let merged = "04-02 12:34:56.789  1234  5678 E AndroidRuntime: FATAL EXCEPTION: main\nProcess: com.example.app, PID: 1234\njava.lang.NullPointerException: Attempt to invoke\n\tat com.app.Foo.bar(Foo.java:12)\n\tat com.app.Baz.qux(Baz.java:34)\nCaused by: java.lang.IllegalStateException: bad\n\tat com.app.Inner.run(Inner.java:5)";
    let entry = LogEntry::parse(merged).unwrap();
    let d = CrashDetector::new();
    let info = d.parse_crash(&entry, CrashType::FatalException);
    assert_eq!(info.headline, "FATAL EXCEPTION: main");
    assert_eq!(info.exception.as_deref(), Some("java.lang.NullPointerException"));
    assert_eq!(info.stack.len(), 4);
    assert!(info.stack[0].starts_with("at "));
    assert!(info.stack[2].starts_with("Caused by:"));
}

#[test]
fn test_parse_anr_no_stack() {
    let line = "04-02 12:34:56.789  1234  5678 E ActivityManager: ANR in com.example.app (com.example.app/.MainActivity)";
    let entry = LogEntry::parse(line).unwrap();
    let d = CrashDetector::new();
    let info = d.parse_crash(&entry, CrashType::Anr);
    assert!(info.headline.contains("ANR in"));
    assert!(info.stack.is_empty());
    assert!(info.exception.is_none());
}
