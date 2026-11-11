//! Highlight group and match-jump tests.

use super::*;
use crate::highlight_model::HighlightGroup;
use std::sync::mpsc;

fn row_with_msg(tag: &str, msg: &str) -> EntryRow {
    EntryRow::from_line(&format!("04-02 10:00:00.000  1234  5678 I {tag}   : {msg}")).unwrap()
}

#[test]
fn test_find_match_next_prev_and_no_wrap() {
    let mut app = App::new(100);
    let (tx, rx) = mpsc::channel();
    tx.send(row_with_msg("T", "aaa")).unwrap();
    tx.send(row_with_msg("T", "hit one")).unwrap();
    tx.send(row_with_msg("T", "bbb")).unwrap();
    tx.send(row_with_msg("T", "hit two")).unwrap();
    drop(tx);
    app.drain(&rx);
    app.following = false;
    app.cursor = 0;
    app.push_or_find_highlight_group(HighlightGroup::from_pattern("hit").unwrap());

    assert_eq!(app.find_match(1), FindJumpResult::Moved);
    assert_eq!(app.cursor, 1);
    assert_eq!(app.find_match(1), FindJumpResult::Moved);
    assert_eq!(app.cursor, 3);
    assert_eq!(app.find_match(1), FindJumpResult::NoMore); // no wrap
    assert_eq!(app.cursor, 3);
    app.cursor = 1;
    assert_eq!(app.find_match(-1), FindJumpResult::NoMore); // no wrap backward
    assert_eq!(app.cursor, 1);
}

#[test]
fn test_find_match_noop_without_search() {
    let mut app = App::new(100);
    let (tx, rx) = mpsc::channel();
    tx.send(row_with_msg("T", "x")).unwrap();
    drop(tx);
    app.drain(&rx);
    assert_eq!(app.find_match(1), FindJumpResult::None);
}

#[test]
fn test_jump_first_match_and_highlight_match_stats() {
    let mut app = App::new(100);
    let (tx, rx) = mpsc::channel();
    tx.send(row_with_msg("T", "aaa")).unwrap();
    tx.send(row_with_msg("T", "hit one")).unwrap();
    tx.send(row_with_msg("T", "bbb")).unwrap();
    tx.send(row_with_msg("T", "hit two")).unwrap();
    drop(tx);
    app.drain(&rx);
    app.following = true;
    app.cursor = 3;
    assert!(app.highlight_match_stats().is_none());

    app.push_or_find_highlight_group(HighlightGroup::from_pattern("hit").unwrap());
    app.cursor = 0; // non-hit row
    assert_eq!(app.highlight_match_stats(), Some((None, 2)));

    assert!(app.jump_first_match());
    assert_eq!(app.cursor, 1);
    assert!(!app.following);
    assert_eq!(app.highlight_match_stats(), Some((Some(1), 2)));

    app.cursor = 3;
    assert_eq!(app.highlight_match_stats(), Some((Some(2), 2)));

    app.cursor = 2;
    assert_eq!(app.highlight_match_stats(), Some((None, 2)));
}

#[test]
fn test_jump_first_match_noop_when_no_hits() {
    let mut app = App::new(100);
    let (tx, rx) = mpsc::channel();
    tx.send(row_with_msg("T", "aaa")).unwrap();
    drop(tx);
    app.drain(&rx);
    app.cursor = 0;
    app.push_or_find_highlight_group(HighlightGroup::from_pattern("zzz").unwrap());
    assert!(!app.jump_first_match());
    assert_eq!(app.cursor, 0);
    assert_eq!(app.highlight_match_stats(), Some((None, 0)));
}

#[test]
fn test_jump_first_match_targets_newest_group_only() {
    let mut app = App::new(100);
    let (tx, rx) = mpsc::channel();
    tx.send(row_with_msg("T", "foo early")).unwrap();
    tx.send(row_with_msg("T", "bar later")).unwrap();
    tx.send(row_with_msg("T", "foo late")).unwrap();
    drop(tx);
    app.drain(&rx);
    app.following = false;
    app.cursor = 0;

    app.push_or_find_highlight_group(HighlightGroup::from_pattern("foo").unwrap());
    app.push_or_find_highlight_group(HighlightGroup::from_pattern("bar").unwrap());
    assert_eq!(app.active_highlight, Some(1));

    assert!(app.jump_first_match());
    // Must land on newest group ("bar"), not the earlier "foo" at index 0.
    assert_eq!(app.cursor, 1);
    assert_eq!(app.current_row().unwrap().msg, "bar later");
}

#[test]
fn test_find_match_ignore_case_by_default() {
    let mut app = App::new(100);
    let (tx, rx) = mpsc::channel();
    tx.send(row_with_msg("T", "an error occurred")).unwrap();
    drop(tx);
    app.drain(&rx);
    app.following = false;
    app.cursor = 0;
    app.push_or_find_highlight_group(HighlightGroup::from_pattern("ERROR").unwrap());
    assert!(app.highlight_groups.any_match("", "an error occurred"));
}

#[test]
fn test_find_match_hits_tag_only() {
    let mut app = App::new(100);
    let (tx, rx) = mpsc::channel();
    tx.send(row_with_msg("Other", "aaa")).unwrap();
    tx.send(row_with_msg("MyTag", "bbb")).unwrap();
    tx.send(row_with_msg("Other", "ccc")).unwrap();
    drop(tx);
    app.drain(&rx);
    app.following = false;
    app.cursor = 0;
    app.push_or_find_highlight_group(HighlightGroup::from_pattern("MyTag").unwrap());
    assert_eq!(app.highlight_match_stats(), Some((None, 1)));
    assert!(app.jump_first_match());
    assert_eq!(app.cursor, 1);
    assert_eq!(app.current_row().unwrap().tag, "MyTag");
    assert_eq!(app.find_match(1), FindJumpResult::NoMore);
    assert_eq!(app.cursor, 1, "only one tag hit; no wrap — stay put");
}

#[test]
fn test_disabled_search_group_excluded_from_find() {
    let mut app = App::new(100);
    let (tx, rx) = mpsc::channel();
    tx.send(row_with_msg("T", "hit")).unwrap();
    drop(tx);
    app.drain(&rx);
    app.push_or_find_highlight_group(HighlightGroup::from_pattern("hit").unwrap());
    app.highlight_groups.groups[0].enabled = false;
    assert_eq!(app.find_match(1), FindJumpResult::None);
    assert!(app.highlight_match_stats().is_none());
}

#[test]
fn test_push_or_find_highlight_group_dedups() {
    let mut app = App::new(100);
    let idx0 = app.push_or_find_highlight_group(HighlightGroup::from_pattern("foo").unwrap());
    assert_eq!(idx0, 0);
    assert_eq!(app.active_highlight, Some(0));
    app.push_or_find_highlight_group(HighlightGroup::from_pattern("bar").unwrap());
    assert_eq!(app.active_highlight, Some(1));
    let idx1 = app.push_or_find_highlight_group(HighlightGroup::from_pattern("FOO").unwrap());
    assert_eq!(idx1, 0);
    assert_eq!(app.active_highlight, Some(0));
    assert_eq!(app.highlight_groups.groups.len(), 2);
}

#[test]
fn test_open_highlight_finder_empty_is_new_else_manage() {
    use crate::picker::{PickerKind, PickerMode};
    let mut app = App::new(100);
    app.open_highlight_finder();
    assert_eq!(app.picker.as_ref().unwrap().kind, PickerKind::Highlight);
    assert_eq!(app.picker.as_ref().unwrap().mode, PickerMode::New);
    app.close_picker();
    app.push_or_find_highlight_group(HighlightGroup::from_pattern("error").unwrap());
    app.open_highlight_finder();
    assert_eq!(app.picker.as_ref().unwrap().mode, PickerMode::Manage);
    assert!(!app.picker.as_ref().unwrap().auto_from_manage);
}

#[test]
fn test_activate_highlight_group_enables_then_jumps() {
    let mut app = App::new(100);
    let (tx, rx) = mpsc::channel();
    tx.send(row_with_msg("T", "aaa")).unwrap();
    tx.send(row_with_msg("T", "error here")).unwrap();
    tx.send(row_with_msg("T", "warn too")).unwrap();
    drop(tx);
    app.drain(&rx);
    app.following = false;
    app.cursor = 0;
    app.push_or_find_highlight_group(HighlightGroup::from_pattern("error").unwrap());
    app.push_or_find_highlight_group(HighlightGroup::from_pattern("warn").unwrap());
    app.highlight_groups.groups[0].enabled = false;
    assert!(app.activate_highlight_group(0));
    assert!(app.highlight_groups.groups[0].enabled);
    assert!(app.highlight_groups.groups[1].enabled);
    assert_eq!(app.active_highlight, Some(0));
    assert_eq!(app.cursor, 1);
    assert!(!app.following);
    assert!(!app.view_focus.highlight);
}

#[test]
fn test_find_match_only_uses_active_highlight_group() {
    let mut app = App::new(100);
    let (tx, rx) = mpsc::channel();
    tx.send(row_with_msg("T", "foo early")).unwrap();
    tx.send(row_with_msg("T", "bar mid")).unwrap();
    tx.send(row_with_msg("T", "foo late")).unwrap();
    drop(tx);
    app.drain(&rx);
    app.following = false;
    app.cursor = 0;
    app.push_or_find_highlight_group(HighlightGroup::from_pattern("foo").unwrap());
    app.push_or_find_highlight_group(HighlightGroup::from_pattern("bar").unwrap());
    // active is "bar" — n must not land on "foo"
    assert_eq!(app.find_match(1), FindJumpResult::Moved);
    assert_eq!(app.current_row().unwrap().msg, "bar mid");
    assert_eq!(app.find_match(1), FindJumpResult::NoMore); // no wrap
    assert_eq!(app.current_row().unwrap().msg, "bar mid");
}

#[test]
fn test_delete_active_highlight_falls_back_to_newest() {
    let mut app = App::new(100);
    app.push_or_find_highlight_group(HighlightGroup::from_pattern("a").unwrap());
    app.push_or_find_highlight_group(HighlightGroup::from_pattern("b").unwrap());
    app.push_or_find_highlight_group(HighlightGroup::from_pattern("c").unwrap());
    assert_eq!(app.active_highlight, Some(2));
    app.highlight_cursor = 2;
    app.focus = Focus::HighlightStrip;
    app.delete_focused_strip_group(StripKind::Highlight);
    assert_eq!(app.highlight_groups.groups.len(), 2);
    assert_eq!(app.active_highlight, Some(1)); // newest remaining ("b")
    app.highlight_cursor = 0;
    app.delete_focused_strip_group(StripKind::Highlight); // remove "a", active was 1 -> shifts to 0
    assert_eq!(app.active_highlight, Some(0));
    assert_eq!(app.highlight_groups.groups[0].pattern, "b");
}
