use aloggrep::sampler::Sampler;

#[test]
fn test_passthrough() {
    let mut s = Sampler::new(0, 0, 0);
    assert!(s.should_emit("line1"));
    assert!(s.should_emit("line2"));
    assert!(!s.needs_full_scan());
    let r = s.finish();
    assert!(r.lines.is_empty());
    assert!(r.header.is_none());
}

#[test]
fn test_tail_only() {
    let mut s = Sampler::new(2, 0, 0);
    assert!(!s.should_emit("line1"));
    assert!(!s.should_emit("line2"));
    assert!(!s.should_emit("line3"));
    assert!(!s.should_emit("line4"));
    assert!(s.needs_full_scan());
    let r = s.finish();
    assert_eq!(r.lines, vec!["line3", "line4"]);
    assert!(r.header.as_ref().unwrap().contains("showing last 2 of 4"));
}

#[test]
fn test_head_tail() {
    let mut s = Sampler::new(2, 0, 2);
    assert!(s.should_emit("line1"));
    assert!(s.should_emit("line2"));
    assert!(!s.should_emit("line3"));
    assert!(!s.should_emit("line4"));
    assert!(!s.should_emit("line5"));
    let r = s.finish();
    assert_eq!(r.lines, vec!["line4", "line5"]);
    assert!(r.header.as_ref().unwrap().contains("1 entries omitted"));
}

#[test]
fn test_head_tail_no_skip() {
    let mut s = Sampler::new(2, 0, 2);
    assert!(s.should_emit("line1"));
    assert!(s.should_emit("line2"));
    assert!(!s.should_emit("line3"));
    assert!(!s.should_emit("line4"));
    let r = s.finish();
    assert_eq!(r.lines, vec!["line3", "line4"]);
    assert!(r.header.is_none());
}

#[test]
fn test_reservoir() {
    let mut s = Sampler::new(0, 3, 0);
    for i in 0..100 {
        assert!(!s.should_emit(&format!("line{i}")));
    }
    assert!(s.needs_full_scan());
    let r = s.finish();
    assert_eq!(r.lines.len(), 3);
    assert!(r.header.as_ref().unwrap().contains("sampled 3 of 100"));
}

#[test]
fn test_reservoir_small_input() {
    let mut s = Sampler::new(0, 10, 0);
    for i in 0..3 {
        s.should_emit(&format!("line{i}"));
    }
    let r = s.finish();
    assert_eq!(r.lines, vec!["line0", "line1", "line2"]);
    assert!(r.header.is_none());
}

#[test]
fn test_reservoir_order_preserved() {
    let mut s = Sampler::new(0, 5, 0);
    for i in 0..5 {
        s.should_emit(&format!("line{i}"));
    }
    let r = s.finish();
    assert_eq!(r.lines, vec!["line0", "line1", "line2", "line3", "line4"]);
}
