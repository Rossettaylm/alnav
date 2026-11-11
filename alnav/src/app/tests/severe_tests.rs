//! Severe-row jump tests.

use super::*;
use crate::filter_model::Group;
use std::sync::mpsc;

fn row_level(level: char, tag: &str, msg: &str) -> EntryRow {
    EntryRow::from_line(&format!(
        "04-02 10:00:00.000  1234  5678 {level} {tag}   : {msg}"
    ))
    .unwrap()
}

fn tag_group(tag: &str) -> Group {
    use crate::fuzzy::SameFieldOp;
    use crate::input::{Chip, ChipField};
    Group {
        label: format!("tag:{tag}"),
        chips: vec![Chip {
            field: ChipField::Tag,
            value: tag.into(),
        }],
        enabled: true,
        same_field_op: SameFieldOp::And,
    }
}

fn filter_group(label: &str, tag: &str) -> Group {
    let mut g = tag_group(tag);
    g.label = label.into();
    g
}

#[test]
fn test_find_severe_next_prev_and_no_wrap() {
    let mut app = App::new(100);
    let (tx, rx) = mpsc::channel();
    tx.send(row_level('I', "T", "ok")).unwrap();
    tx.send(row_level('E', "T", "err one")).unwrap();
    tx.send(row_level('I', "T", "ok2")).unwrap();
    tx.send(row_level('F', "T", "fatal")).unwrap();
    drop(tx);
    app.drain(&rx);
    app.following = false;
    app.cursor = 0;

    assert_eq!(app.find_severe(1), FindJumpResult::Moved);
    assert_eq!(app.cursor, 1);
    assert!(!app.following);
    assert_eq!(app.find_severe(1), FindJumpResult::Moved);
    assert_eq!(app.cursor, 3);
    assert_eq!(app.find_severe(1), FindJumpResult::NoMore); // no wrap
    assert_eq!(app.cursor, 3);
    app.cursor = 1;
    assert_eq!(app.find_severe(-1), FindJumpResult::NoMore); // no wrap backward
    assert_eq!(app.cursor, 1);
}

#[test]
fn test_find_severe_noop_when_none() {
    let mut app = App::new(100);
    let (tx, rx) = mpsc::channel();
    tx.send(row_level('I', "T", "ok")).unwrap();
    drop(tx);
    app.drain(&rx);
    app.following = true;
    app.cursor = 0;
    assert_eq!(app.find_severe(1), FindJumpResult::None);
    assert_eq!(app.cursor, 0);
    assert!(app.following, "no jump must not clear following");
}

#[test]
fn test_find_severe_respects_visible_filter() {
    let mut app = App::new(100);
    let (tx, rx) = mpsc::channel();
    tx.send(row_level('E', "Keep", "err keep")).unwrap();
    tx.send(row_level('E', "Drop", "err drop")).unwrap();
    tx.send(row_level('I', "Keep", "ok")).unwrap();
    drop(tx);
    app.drain(&rx);

    assert!(app.push_filter_group(filter_group("tag=Keep", "Keep")));
    app.rebuild_visible();
    assert_eq!(app.index.visible.len(), 2);
    app.following = false;
    app.cursor = 0; // on Keep E

    assert_eq!(app.find_severe(1), FindJumpResult::NoMore);
    // Only one severe in visible; no wrap — stay on Keep E.
    assert_eq!(app.cursor, 0);
    assert_eq!(app.current_row().unwrap().tag, "Keep");
}

#[test]
fn test_find_severe_hits_crash_message_even_if_info_level() {
    let mut app = App::new(100);
    let (tx, rx) = mpsc::channel();
    tx.send(row_level('I', "T", "normal")).unwrap();
    tx.send(row_level('I', "AndroidRuntime", "FATAL EXCEPTION: main"))
        .unwrap();
    tx.send(row_level('I', "T", "after")).unwrap();
    drop(tx);
    app.drain(&rx);
    app.following = false;
    app.cursor = 0;
    assert!(is_severe_row(
        &app.view_source()[app.source_idx_for_visible(1).unwrap()]
    ));
    assert_eq!(app.find_severe(1), FindJumpResult::Moved);
    assert_eq!(app.cursor, 1);
}
