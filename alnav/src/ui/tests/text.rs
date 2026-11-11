//! Editable text span windowing tests.

use super::*;
use unicode_width::UnicodeWidthStr;

#[test]
fn editable_text_spans_follow_caret_when_truncated() {
    let (spans, caret_col) = editable_text_spans("abcdefghij", 10, Some(5));
    let joined: String = spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(
        joined.contains('j'),
        "end char must remain visible: {joined:?}"
    );
    assert!(
        !joined.contains('a'),
        "start char should scroll off: {joined:?}"
    );
    assert_eq!(caret_col as usize, UnicodeWidthStr::width(joined.as_str()));
}

#[test]
fn editable_text_spans_mid_caret_is_plain_text() {
    let (spans, caret_col) = editable_text_spans("abcdef", 2, None);
    let joined: String = spans.iter().map(|s| s.content.as_ref()).collect();
    assert_eq!(joined, "abcdef");
    assert_eq!(caret_col, 2);
    assert!(!joined.contains('▏'));
}

#[test]
fn editable_text_spans_start_caret_col_is_zero() {
    let (spans, caret_col) = editable_text_spans("ab", 0, None);
    let joined: String = spans.iter().map(|s| s.content.as_ref()).collect();
    assert_eq!(joined, "ab");
    assert_eq!(caret_col, 0);
}
