//! Follow-mode tests.

use super::*;
use crate::filter_model::Group;
use std::sync::mpsc;

fn row(tag: &str) -> EntryRow {
    EntryRow::from_line(&format!("04-02 10:00:00.000  1  1 I {tag}   : m")).unwrap()
}

#[test]
fn test_follow_pins_cursor_to_latest() {
    let mut app = App::new(100);
    let (tx, rx) = mpsc::channel();
    tx.send(row("A")).unwrap();
    tx.send(row("B")).unwrap();
    drop(tx);
    app.drain(&rx);
    assert_eq!(app.cursor, 1); // pinned to last row
}

#[test]
fn test_manual_up_navigation_pauses_follow() {
    let mut app = App::new(100);
    let (tx, rx) = mpsc::channel();
    tx.send(row("A")).unwrap();
    tx.send(row("B")).unwrap();
    drop(tx);
    app.drain(&rx);

    app.move_cursor_manual(-1);
    assert!(!app.following);

    let (tx2, rx2) = mpsc::channel();
    tx2.send(row("C")).unwrap();
    drop(tx2);
    app.drain(&rx2);
    assert_eq!(app.cursor, 0); // did not jump to the new bottom
}

#[test]
fn test_resume_following_pins_bottom() {
    let mut app = App::new(100);
    let (tx, rx) = mpsc::channel();
    tx.send(row("A")).unwrap();
    tx.send(row("B")).unwrap();
    drop(tx);
    app.drain(&rx);
    app.move_cursor_manual(-1);
    assert!(!app.following);
    app.resume_following();
    assert!(app.following);
    assert_eq!(app.cursor, 1);
}

#[test]
fn test_manual_move_away_from_bottom_pauses_follow() {
    let mut app = App::new(100);
    let (tx, rx) = mpsc::channel();
    tx.send(row("A")).unwrap();
    tx.send(row("B")).unwrap();
    tx.send(row("C")).unwrap();
    drop(tx);
    app.drain(&rx);
    assert!(app.following);
    app.move_cursor_manual(-1);
    assert!(!app.following);
    assert_eq!(app.cursor, 1);
}

#[test]
fn test_manual_down_to_bottom_resumes_follow() {
    let mut app = App::new(100);
    let (tx, rx) = mpsc::channel();
    tx.send(row("A")).unwrap();
    tx.send(row("B")).unwrap();
    tx.send(row("C")).unwrap();
    drop(tx);
    app.drain(&rx);
    app.move_cursor_manual(-2);
    assert!(!app.following);
    assert_eq!(app.cursor, 0);
    app.move_cursor_manual(1);
    assert!(!app.following, "mid-list down must not resume");
    assert_eq!(app.cursor, 1);
    app.move_cursor_manual(1);
    assert!(app.following, "landing on bottom resumes following");
    assert_eq!(app.cursor, 2);
}

#[test]
fn test_manual_down_while_already_at_bottom_keeps_follow() {
    let mut app = App::new(100);
    let (tx, rx) = mpsc::channel();
    tx.send(row("A")).unwrap();
    tx.send(row("B")).unwrap();
    drop(tx);
    app.drain(&rx);
    assert!(app.following);
    assert_eq!(app.cursor, 1);
    app.move_cursor_manual(1); // clamp at bottom
    assert!(app.following);
    assert_eq!(app.cursor, 1);
}

#[test]
fn test_rebuild_visible_follows_when_following_and_visible_set_grows() {
    let mut app = App::new(100);
    app.groups = GroupList {
        groups: vec![Group {
            label: "a".into(),
            chips: vec![crate::input::Chip {
                field: crate::input::ChipField::Tag,
                value: "A".into(),
            }],
            enabled: true,
            same_field_op: crate::fuzzy::SameFieldOp::And,
        }],
        excludes: Vec::new(),
    };
    let (tx, rx) = mpsc::channel();
    tx.send(row("A")).unwrap();
    tx.send(row("B")).unwrap(); // doesn't match "a" group, filtered out
    drop(tx);
    app.drain(&rx);
    assert_eq!(app.visible, Visible::All { len: 1 }); // only A is visible
    assert!(app.following);

    app.groups.groups.clear(); // simulates deleting the last filter group -> empty GroupList matches everything
    app.rebuild_visible();
    assert_eq!(app.visible, Visible::All { len: 2 }); // both now visible, set grew
    assert_eq!(app.cursor, 1); // still following: cursor pinned to new bottom (B), not stuck at old position
}
