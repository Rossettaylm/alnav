use aloggrep::histogram::{parse_interval, Histogram};
use aloggrep::parser::LogEntry;

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
fn test_histogram_json_output() {
    let mut h = Histogram::new(60);
    let line = "04-02 10:32:05.000  1234  5678 E Tag     : err";
    h.record(&LogEntry::parse(line).unwrap());

    let mut buf = Vec::new();
    h.write_json(&mut buf).unwrap();
    let json = String::from_utf8(buf).unwrap();
    assert!(json.contains("\"bucket\":\"04-02 10:32:00\""));
    assert!(json.contains("\"E\":1"));
    assert!(json.contains("\"anomaly\":false"));
    assert!(json.contains("\"_stats\""));
}

#[test]
fn test_histogram_anomaly_detection() {
    let mut h = Histogram::new(60);
    // 5 normal buckets with 1 error each
    for i in 0..5 {
        let line = format!("04-02 10:{:02}:05.000  1234  5678 E Tag     : err", 30 + i);
        h.record(&LogEntry::parse(&line).unwrap());
    }
    // 1 spike bucket with 20 errors
    for _ in 0..20 {
        let line = "04-02 10:35:05.000  1234  5678 E Tag     : err";
        h.record(&LogEntry::parse(line).unwrap());
    }
    let mut buf = Vec::new();
    h.write_json(&mut buf).unwrap();
    let json = String::from_utf8(buf).unwrap();
    assert!(json.contains("\"anomaly\":true"));
    assert!(json.contains("\"anomaly\":false"));
    assert!(json.contains("\"_stats\""));
    assert!(json.contains("\"spike_buckets\""));
    assert!(json.contains("04-02 10:35:00"));
}

#[test]
fn test_histogram_no_anomaly_uniform() {
    let mut h = Histogram::new(60);
    // All buckets have exactly 1 error -> stddev = 0 -> no anomalies
    for i in 0..3 {
        let line = format!("04-02 10:{:02}:05.000  1234  5678 E Tag     : err", 30 + i);
        h.record(&LogEntry::parse(&line).unwrap());
    }
    let mut buf = Vec::new();
    h.write_json(&mut buf).unwrap();
    let json = String::from_utf8(buf).unwrap();
    assert!(!json.contains("\"anomaly\":true"));
    assert!(json.contains("\"anomaly\":false"));
}
