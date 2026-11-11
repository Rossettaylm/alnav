//! Focus cycling tests.

use super::*;
use crate::filter_model::Group;

fn g(label: &str) -> Group {
    Group {
        label: label.into(),
        chips: Vec::new(),
        enabled: true,
        same_field_op: crate::fuzzy::SameFieldOp::And,
    }
}

#[test]
fn test_cycle_focus_forward_wraps() {
    let mut app = App::new(100);
    assert_eq!(app.focus, Focus::LogList);
    app.cycle_focus_forward();
    assert_eq!(app.focus, Focus::Input);
    app.cycle_focus_forward();
    assert_eq!(app.focus, Focus::ChipStrip);
    app.cycle_focus_forward();
    assert_eq!(app.focus, Focus::ExcludeStrip);
    app.cycle_focus_forward();
    assert_eq!(app.focus, Focus::HighlightStrip);
    app.cycle_focus_forward();
    assert_eq!(app.focus, Focus::LogList);
}

#[test]
fn test_delete_focused_group_removes_and_rescans() {
    let mut app = App::new(100);
    app.groups.groups.push(g("g0"));
    app.groups.groups.push(g("g1"));
    app.group_cursor = 0;
    app.delete_focused_group();
    assert_eq!(app.groups.groups.len(), 1);
    assert_eq!(app.groups.groups[0].label, "g1");
}

#[test]
fn test_delete_focused_group_returns_focus_to_loglist_when_list_becomes_empty() {
    let mut app = App::new(100);
    app.groups.groups.push(g("g0"));
    app.focus = Focus::ChipStrip;
    app.delete_focused_group();
    assert!(app.groups.groups.is_empty());
    assert_eq!(app.focus, Focus::LogList);
}

#[test]
fn test_delete_focused_group_keeps_focus_when_groups_remain() {
    let mut app = App::new(100);
    app.groups.groups.push(g("g0"));
    app.groups.groups.push(g("g1"));
    app.focus = Focus::ChipStrip;
    app.delete_focused_group();
    assert!(!app.groups.groups.is_empty());
    assert_eq!(
        app.focus,
        Focus::ChipStrip,
        "focus should stay put while groups remain"
    );
}

#[test]
fn test_move_group_cursor_clamps() {
    let mut app = App::new(100);
    app.groups.groups.push(g("g0"));
    app.move_group_cursor(-5);
    assert_eq!(app.group_cursor, 0);
    app.move_group_cursor(5);
    assert_eq!(app.group_cursor, 0);
}

#[test]
fn test_toggle_disable_filter_rebuilds_visible() {
    let mut app = App::new(100);
    app.groups.groups.push(Group {
        label: "a".into(),
        chips: vec![crate::input::Chip {
            field: crate::input::ChipField::Tag,
            value: "A".into(),
        }],
        enabled: true,
        same_field_op: crate::fuzzy::SameFieldOp::And,
    });
    let (tx, rx) = std::sync::mpsc::channel();
    tx.send(EntryRow::from_line("04-02 10:00:00.000  1  1 I A   : m").unwrap())
        .unwrap();
    tx.send(EntryRow::from_line("04-02 10:00:00.000  1  1 I B   : m").unwrap())
        .unwrap();
    drop(tx);
    app.drain(&rx);
    assert_eq!(app.index.visible.len(), 1);
    app.toggle_disable_focused(StripKind::Filter);
    assert!(!app.groups.groups[0].enabled);
    assert_eq!(
        app.index.visible.len(),
        2,
        "disabled-only list ≡ empty filter"
    );
}
