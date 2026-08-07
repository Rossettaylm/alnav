use alnav::parser::LogEntry;
use alnav::summary::Summary;

#[test]
fn test_summary_basic() {
    let mut s = Summary::new();
    let l1 = "04-02 10:00:00.000  1234  5678 E OkHttp  : timeout after 100ms";
    let l2 = "04-02 10:01:00.000  1234  5678 W Retrofit: slow response";
    s.record(&LogEntry::parse(l1).unwrap());
    s.record(&LogEntry::parse(l2).unwrap());

    let json = s.to_json(2);
    assert!(json.contains("\"total\":2"));
    assert!(json.contains("\"matched\":2"));
    assert!(json.contains("\"tag\":\"OkHttp\""));
    assert!(json.contains("\"tag\":\"Retrofit\""));
}

#[test]
fn test_summary_tag_levels() {
    let mut s = Summary::new();
    for _ in 0..3 {
        let l = "04-02 10:00:00.000  1234  5678 I OkHttp  : request";
        s.record(&LogEntry::parse(l).unwrap());
    }
    for _ in 0..2 {
        let l = "04-02 10:00:01.000  1234  5678 E OkHttp  : timeout";
        s.record(&LogEntry::parse(l).unwrap());
    }
    let json = s.to_json(5);
    assert!(json.contains("\"tag\":\"OkHttp\""));
    assert!(json.contains("\"count\":5"));
    assert!(json.contains("\"I\":3"));
    assert!(json.contains("\"E\":2"));
}

#[test]
fn test_summary_top_errors() {
    let mut s = Summary::new();
    for i in 0..5 {
        let line = format!("04-02 10:00:0{i}.000  1234  5678 E OkHttp  : timeout after {i}00ms");
        s.record(&LogEntry::parse(&line).unwrap());
    }
    let line = "04-02 10:01:00.000  1234  5678 E DB      : connection lost";
    s.record(&LogEntry::parse(line).unwrap());

    let json = s.to_json(6);
    assert!(json.contains("timeout after <N>ms"));
    assert!(json.contains("\"count\":5"));
    assert!(json.contains("connection lost"));
}

#[test]
fn test_summary_crash_count() {
    let mut s = Summary::new();
    let l1 = "04-02 10:00:00.000  1234  5678 E AndroidRuntime: FATAL EXCEPTION: main";
    let l2 = "04-02 10:01:00.000  1234  5678 E ActivityManager: ANR in com.app";
    let l3 = "04-02 10:02:00.000  1234  5678 W OkHttp  : timeout";
    s.record(&LogEntry::parse(l1).unwrap());
    s.record(&LogEntry::parse(l2).unwrap());
    s.record(&LogEntry::parse(l3).unwrap());

    let json = s.to_json(3);
    assert!(json.contains("\"crashes\":2"));
}

#[test]
fn test_summary_ignores_warnings_in_errors() {
    let mut s = Summary::new();
    let line = "04-02 10:00:00.000  1234  5678 W Tag     : some warning";
    s.record(&LogEntry::parse(line).unwrap());

    let json = s.to_json(1);
    assert!(json.contains("\"top_errors\":[]"));
}

fn build_sample_summary() -> Summary {
    let mut s = Summary::new();
    let l1 = "04-02 10:00:00.000  1234  5678 E OkHttp  : timeout after 100ms";
    let l2 = "04-02 10:01:00.000  1234  5678 W Retrofit: slow response";
    let l3 = "04-02 10:02:00.000  1234  5678 E AndroidRuntime: FATAL EXCEPTION: main";
    s.record(&LogEntry::parse(l1).unwrap());
    s.record(&LogEntry::parse(l2).unwrap());
    s.record(&LogEntry::parse(l3).unwrap());
    s
}

#[test]
fn test_into_report_matches_to_json() {
    let via_json = build_sample_summary().to_json(3);
    let report = build_sample_summary().into_report(3);
    let via_report = serde_json::to_string(&report).unwrap();

    let mut a: serde_json::Value = serde_json::from_str(&via_json).unwrap();
    let mut b: serde_json::Value = serde_json::from_str(&via_report).unwrap();
    for v in [&mut a, &mut b] {
        if let Some(arr) = v.get_mut("top_tags").and_then(|x| x.as_array_mut()) {
            arr.sort_by(|x, y| x["tag"].as_str().cmp(&y["tag"].as_str()));
        }
        if let Some(arr) = v.get_mut("top_errors").and_then(|x| x.as_array_mut()) {
            arr.sort_by(|x, y| x["pattern"].as_str().cmp(&y["pattern"].as_str()));
        }
    }
    assert_eq!(a, b);
}

#[test]
fn test_into_report_fields() {
    let report = build_sample_summary().into_report(3);
    assert_eq!(report.total, 3);
    assert_eq!(report.matched, 3);
    assert_eq!(report.crashes, 1);
    assert!(report.top_tags.iter().any(|t| t.tag == "OkHttp"));
    assert!(report.top_errors.iter().any(|e| e.tag == "AndroidRuntime"));
}
