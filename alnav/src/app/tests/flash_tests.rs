//! Flash toast and related flow tests.

use super::*;
use std::time::{Duration, Instant};

#[test]
fn set_flash_stores_msg_and_deadline() {
    let mut app = App::new(100);
    app.set_flash("YANKED");
    assert_eq!(app.status_msg.as_deref(), Some("YANKED"));
    assert!(app.status_flash_until.is_some());
}

#[test]
fn tick_flash_clears_expired() {
    let mut app = App::new(100);
    app.set_flash("NO ERROR");
    app.status_flash_until = Some(Instant::now() - Duration::from_millis(1));
    app.tick_flash();
    assert!(app.status_msg.is_none());
    assert!(app.status_flash_until.is_none());
}

#[test]
fn tick_flash_keeps_unexpired() {
    let mut app = App::new(100);
    app.set_flash("FILTER");
    app.tick_flash();
    assert_eq!(app.status_msg.as_deref(), Some("FILTER"));
}

#[test]
fn cancel_pending_does_not_clear_flash() {
    let mut app = App::new(100);
    app.set_flash("YANKED");
    app.begin_bookmark_op();
    app.cancel_bookmark_op();
    assert_eq!(app.status_msg.as_deref(), Some("YANKED"));
    assert!(!app.pending_m);
}

fn sample_tag_group(tag: &str) -> Group {
    use crate::input::{build_group_from_chips, Chip, ChipField};
    build_group_from_chips(
        vec![Chip {
            field: ChipField::Tag,
            value: tag.to_string(),
        }],
        true,
    )
    .unwrap()
    .unwrap()
}

#[test]
fn update_and_clear_highlight_groups() {
    let mut app = App::new(100);
    let g = HighlightGroup::from_pattern("foo").unwrap();
    app.push_or_find_highlight_group(g);
    assert!(app.update_search_group(0, "bar"));
    assert!(app.highlight_groups.groups[0].same_pattern_as("bar"));
    app.clear_highlight_groups();
    assert!(app.highlight_groups.groups.is_empty());
    assert!(app.active_highlight.is_none());
}

#[test]
fn update_search_group_dedups_other_indices() {
    let mut app = App::new(100);
    app.push_or_find_highlight_group(HighlightGroup::from_pattern("foo").unwrap());
    app.push_or_find_highlight_group(HighlightGroup::from_pattern("bar").unwrap());
    assert!(!app.update_search_group(0, "BAR"));
    assert!(app.highlight_groups.groups[0].same_pattern_as("foo"));
}

#[test]
fn update_and_clear_filter_groups() {
    let mut app = App::new(100);
    assert!(app.push_filter_group(sample_tag_group("A")));
    let g2 = sample_tag_group("B");
    assert!(app.update_filter_group(0, g2));
    assert!(app.groups.groups[0].same_as(&sample_tag_group("B")));
    app.clear_filter_groups();
    assert!(app.groups.groups.is_empty());
}

#[test]
fn delete_filter_group_at_out_of_bounds() {
    let mut app = App::new(100);
    assert!(!app.delete_filter_group_at(0));
}

#[test]
fn clear_bookmarks() {
    let mut app = App::new(100);
    let mut row = crate::model::EntryRow::from_line_or_raw("04-02 10:00:00.000  1  1 I T : x");
    row.row_id = 1;
    app.bookmarks.try_add(Bookmark::from_row(row)).unwrap();
    app.clear_bookmarks();
    assert!(app.bookmarks.is_empty());
    assert!(app.compare.is_none());
}

#[test]
fn bookmark_add_toggles_and_cap_rejects() {
    let mut app = App::new(100);
    let (tx, rx) = std::sync::mpsc::channel();
    for i in 0..18u32 {
        tx.send(
            crate::model::EntryRow::from_line(&format!(
                "04-02 10:00:{i:02}.000  1  1 I Tag{i}   : msg{i}"
            ))
            .unwrap(),
        )
        .unwrap();
    }
    drop(tx);
    app.drain(&rx);
    app.following = false;
    app.cursor = 0;
    app.bookmark_add_current();
    assert_eq!(app.bookmarks.len(), 1);
    assert_eq!(app.status_msg.as_deref(), Some("BOOKMARKED"));
    assert!(app.bookmark_row_ids.contains(&app.view_source()[0].row_id));
    app.bookmark_add_current();
    assert!(app.bookmarks.is_empty());
    assert_eq!(app.status_msg.as_deref(), Some("REMOVED"));
    app.bookmark_add_current();
    for i in 1..16 {
        app.cursor = i;
        app.bookmark_add_current();
        assert_eq!(app.status_msg.as_deref(), Some("BOOKMARKED"), "pin {i}");
    }
    assert_eq!(app.bookmarks.len(), 16);
    app.cursor = 16;
    app.bookmark_add_current();
    assert_eq!(app.bookmarks.len(), 16);
    assert_eq!(app.status_msg.as_deref(), Some("BOOKMARKS FULL"));
}

#[test]
fn open_compare_panel_empty_vs_pins() {
    let mut app = App::new(100);
    app.following = true;
    app.open_compare_panel();
    assert!(app.compare.is_none());
    assert_eq!(app.status_msg.as_deref(), Some("NO BOOKMARKS"));
    assert!(app.following);

    let (tx, rx) = std::sync::mpsc::channel();
    tx.send(crate::model::EntryRow::from_line("04-02 10:00:00.000  1  1 I Tag     : x").unwrap())
        .unwrap();
    drop(tx);
    app.drain(&rx);
    app.cursor = 0;
    app.bookmark_add_current();
    app.following = true;
    app.open_compare_panel();
    assert!(app.compare.is_some());
    assert!(!app.following);
    app.close_compare_panel();
    assert!(app.compare.is_none());
    assert!(!app.following);
}

#[test]
fn compare_enter_jump_and_dd_last_closes() {
    let mut app = App::new(100);
    let (tx, rx) = std::sync::mpsc::channel();
    tx.send(
        crate::model::EntryRow::from_line("04-02 10:00:00.000  1  1 I TagA    : first").unwrap(),
    )
    .unwrap();
    tx.send(
        crate::model::EntryRow::from_line("04-02 10:00:01.000  1  1 I TagB    : second").unwrap(),
    )
    .unwrap();
    drop(tx);
    app.drain(&rx);
    app.following = false;
    app.cursor = 0;
    app.bookmark_add_current();
    app.cursor = 1;
    app.open_compare_panel();
    app.compare_jump_selected();
    assert!(app.compare.is_none());
    assert_eq!(app.focus, Focus::LogList);
    assert_eq!(app.cursor, 0);

    app.open_compare_panel();
    app.compare_delete_selected();
    assert!(app.compare.is_none());
    assert!(app.bookmarks.is_empty());
    assert!(app.bookmark_row_ids.is_empty());
}

#[test]
fn compare_yank_sets_last_yanked_to_raw() {
    let mut app = App::new(100);
    let (tx, rx) = std::sync::mpsc::channel();
    tx.send(
        crate::model::EntryRow::from_line("04-02 10:00:00.000  1  1 I Tag     : hello yank")
            .unwrap(),
    )
    .unwrap();
    drop(tx);
    app.drain(&rx);
    app.cursor = 0;
    app.bookmark_add_current();
    app.open_compare_panel();
    app.compare_yank_selected();
    let yanked = app.last_yanked.as_deref().unwrap_or("");
    assert!(yanked.contains("hello yank"), "{yanked}");
}

#[test]
fn compare_enter_filtered_and_evicted_stay_open() {
    use crate::fuzzy::SameFieldOp;
    use crate::input::{Chip, ChipField};

    let mut app = App::new(100);
    let (tx, rx) = std::sync::mpsc::channel();
    tx.send(crate::model::EntryRow::from_line("04-02 10:00:00.000  1  1 I Keep    : a").unwrap())
        .unwrap();
    tx.send(crate::model::EntryRow::from_line("04-02 10:00:01.000  1  1 I Drop    : b").unwrap())
        .unwrap();
    drop(tx);
    app.drain(&rx);
    app.cursor = 1;
    app.bookmark_add_current();
    app.groups.groups.push(crate::filter_model::Group {
        label: "keep".into(),
        chips: vec![Chip {
            field: ChipField::Tag,
            value: "Keep".into(),
        }],
        enabled: true,
        same_field_op: SameFieldOp::And,
    });
    app.rebuild_visible();
    app.open_compare_panel();
    app.compare_jump_selected();
    assert!(
        app.compare.is_none(),
        "alive filtered pin must jump and close"
    );
    assert_eq!(app.focus, Focus::LogList);
    assert_eq!(app.current_row().unwrap().tag, "Drop");

    let mut gone = crate::model::EntryRow::from_line_or_raw("gone");
    gone.row_id = 999_999;
    app.bookmarks.try_add(Bookmark::from_row(gone)).unwrap();
    app.bookmark_row_ids.insert(999_999);
    app.open_compare_panel();
    if let Some(panel) = app.compare.as_mut() {
        panel.cursor = 1;
    }
    app.compare_jump_selected();
    assert!(app.compare.is_some());
    assert_eq!(app.status_msg.as_deref(), Some("BOOKMARK EVICTED"));
}

#[test]
fn stream_pin_survives_filter_in_time_order() {
    use crate::fuzzy::SameFieldOp;
    use crate::input::{Chip, ChipField};

    let mut app = App::new(100);
    let (tx, rx) = std::sync::mpsc::channel();
    tx.send(crate::model::EntryRow::from_line("04-02 10:00:00.000  1  1 I Keep    : a").unwrap())
        .unwrap();
    tx.send(crate::model::EntryRow::from_line("04-02 10:00:01.000  1  1 I Drop    : b").unwrap())
        .unwrap();
    tx.send(crate::model::EntryRow::from_line("04-02 10:00:02.000  1  1 I Keep    : c").unwrap())
        .unwrap();
    drop(tx);
    app.drain(&rx);
    app.cursor = 1;
    app.bookmark_add_current();
    app.groups.groups.push(crate::filter_model::Group {
        label: "keep".into(),
        chips: vec![Chip {
            field: ChipField::Tag,
            value: "Keep".into(),
        }],
        enabled: true,
        same_field_op: SameFieldOp::And,
    });
    app.rebuild_visible();
    assert_eq!(app.visible_len(), 3);
    assert_eq!(app.row_at(0).unwrap().tag, "Keep");
    assert_eq!(app.row_at(1).unwrap().tag, "Drop");
    assert_eq!(app.row_at(2).unwrap().tag, "Keep");
    let pin_id = app.bookmarks.items[0].row_id();
    assert!(app.visible_idx_for_row_id(pin_id).is_some());

    app.cursor = app.visible_idx_for_row_id(pin_id).unwrap();
    app.bookmark_remove_current();
    assert_eq!(app.visible_len(), 2);
    assert_eq!(app.row_at(0).unwrap().tag, "Keep");
    assert_eq!(app.row_at(1).unwrap().tag, "Keep");
}

#[test]
fn stream_pin_survives_lock() {
    let mut app = App::new(100);
    let (tx, rx) = std::sync::mpsc::channel();
    tx.send(crate::model::EntryRow::from_line("04-02 10:00:00.000  1  1 I TagA    : a").unwrap())
        .unwrap();
    tx.send(crate::model::EntryRow::from_line("04-02 10:00:01.000  2  2 I TagB    : b").unwrap())
        .unwrap();
    drop(tx);
    app.drain(&rx);
    app.cursor = 0;
    app.bookmark_add_current();
    app.lock_pid = Some("2".into());
    app.rebuild_visible();
    assert_eq!(app.visible_len(), 2);
    assert_eq!(app.row_at(0).unwrap().pid, "1");
    assert_eq!(app.row_at(1).unwrap().pid, "2");
}

#[test]
fn stream_evicted_pin_not_reinjected() {
    let mut app = App::new(2);
    let (tx, rx) = std::sync::mpsc::channel();
    tx.send(crate::model::EntryRow::from_line("04-02 10:00:00.000  1  1 I Old     : a").unwrap())
        .unwrap();
    tx.send(crate::model::EntryRow::from_line("04-02 10:00:01.000  1  1 I Mid     : b").unwrap())
        .unwrap();
    app.drain(&rx);
    app.cursor = 0;
    app.bookmark_add_current();
    let pinned_id = app.bookmarks.items[0].row_id();
    tx.send(crate::model::EntryRow::from_line("04-02 10:00:02.000  1  1 I Keep    : c").unwrap())
        .unwrap();
    tx.send(crate::model::EntryRow::from_line("04-02 10:00:03.000  1  1 I Keep    : d").unwrap())
        .unwrap();
    drop(tx);
    app.drain(&rx);
    assert!(!app.bookmark_alive(pinned_id));

    use crate::fuzzy::SameFieldOp;
    use crate::input::{Chip, ChipField};
    app.groups.groups.push(crate::filter_model::Group {
        label: "keep".into(),
        chips: vec![Chip {
            field: ChipField::Tag,
            value: "Keep".into(),
        }],
        enabled: true,
        same_field_op: SameFieldOp::And,
    });
    app.rebuild_visible();
    assert!(app.visible_idx_for_row_id(pinned_id).is_none());
    for i in 0..app.visible_len() {
        assert_ne!(app.row_at(i).unwrap().tag, "Old");
    }
}

#[test]
fn delete_highlight_group_at_fixes_active_highlight() {
    let mut app = App::new(100);
    app.push_or_find_highlight_group(HighlightGroup::from_pattern("a").unwrap());
    app.push_or_find_highlight_group(HighlightGroup::from_pattern("b").unwrap());
    assert_eq!(app.active_highlight, Some(1));
    assert!(app.delete_highlight_group_at(1));
    assert_eq!(app.active_highlight, Some(0));
    assert!(app.delete_highlight_group_at(0));
    assert!(app.active_highlight.is_none());
}
