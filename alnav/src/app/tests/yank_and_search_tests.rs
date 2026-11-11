//! Yank and search tests.

use super::*;
use std::sync::mpsc;

fn row_with_msg(tag: &str, msg: &str) -> EntryRow {
    EntryRow::from_line(&format!("04-02 10:00:00.000  1234  5678 I {tag}   : {msg}")).unwrap()
}

#[test]
fn test_yank_field_extracts_tag_and_msg() {
    let mut app = App::new(100);
    let (tx, rx) = mpsc::channel();
    tx.send(row_with_msg("MyTag", "hello")).unwrap();
    drop(tx);
    app.drain(&rx);
    assert_eq!(app.yank_field(YankField::Tag).as_deref(), Some("MyTag"));
    assert_eq!(app.yank_field(YankField::Msg).as_deref(), Some("hello"));
    assert_eq!(app.yank_field(YankField::Pid).as_deref(), Some("1234"));
    assert_eq!(app.yank_field(YankField::Tid).as_deref(), Some("5678"));
    assert_eq!(app.yank_field(YankField::Level).as_deref(), Some("I"));
    assert_eq!(
        app.yank_field(YankField::Timestamp).as_deref(),
        Some("04-02 10:00:00.000")
    );
}

#[test]
fn test_yank_range_joins_raw_with_newlines() {
    let mut app = App::new(100);
    let (tx, rx) = mpsc::channel();
    let a = row_with_msg("A", "one");
    let b = row_with_msg("B", "two");
    let raw_a = a.raw.clone();
    let raw_b = b.raw.clone();
    tx.send(a).unwrap();
    tx.send(b).unwrap();
    drop(tx);
    app.drain(&rx);
    let text = app.yank_range(0, 1, YankField::Raw).unwrap();
    assert_eq!(text, format!("{raw_a}\n{raw_b}"));
}

#[test]
fn test_selection_range_orders_anchor_and_cursor() {
    let mut app = App::new(100);
    let (tx, rx) = mpsc::channel();
    for i in 0..5 {
        tx.send(row_with_msg("T", &format!("m{i}"))).unwrap();
    }
    drop(tx);
    app.drain(&rx);
    app.following = false;
    app.cursor = 3;
    app.visual_anchor = Some(1);
    assert_eq!(app.selection_range(), Some((1, 3)));
    app.cursor = 0;
    assert_eq!(app.selection_range(), Some((0, 1)));
}
