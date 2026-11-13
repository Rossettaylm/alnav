//! Core `App` behaviour tests (filters, intake, panels, sessions).

use super::*;
use crate::app::Pending;
use crate::filter_model::Group;
use std::sync::mpsc;

fn row(tag: &str) -> EntryRow {
    EntryRow::from_line(&format!("04-02 10:00:00.000  1  1 I {tag}   : m")).unwrap()
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
fn test_mark_live_reconnected_clears_done_and_flashes() {
    let mut app = App::new(100);
    app.ingest_done = true;
    app.mark_live_reconnected();
    assert!(!app.ingest_done);
    assert_eq!(app.status_msg.as_deref(), Some("RECONNECTED"));
}

#[test]
fn test_drain_appends_visible_rows() {
    let mut app = App::new(100);
    let (tx, rx) = mpsc::channel();
    tx.send(row("A")).unwrap();
    tx.send(row("B")).unwrap();
    drop(tx);
    app.drain(&rx);
    assert_eq!(app.index.visible.len(), 2);
}

#[test]
fn test_new_app_has_zero_list_offset() {
    let app = App::new(100);
    assert_eq!(app.list_offset, 0);
}

#[test]
fn open_help_from_exclude_preselects_toc_and_close_keeps_following() {
    let mut app = App::new(100);
    app.focus = Focus::ExcludeStrip;
    app.following = false;
    app.open_help();
    assert!(app.help_open);
    assert!(matches!(
        app.help_view,
        crate::help::HelpView::Home { toc: 1, .. }
    ));
    app.close_help();
    assert!(!app.help_open);
    assert!(!app.following);
}

#[test]
fn help_page_scroll_stops_when_last_line_fills_viewport() {
    let mut app = App::new(100);
    app.open_help();
    app.help_open_page(crate::help::HelpPage::Log);
    app.help_body_view_h = 10;
    let n = crate::help::page_doc_lines(&app, crate::help::HelpPage::Log).len();
    let max = crate::help::page_max_scroll(n, 10);
    app.help_scroll_bottom();
    assert!(matches!(
        app.help_view,
        crate::help::HelpView::Page {
            id: crate::help::HelpPage::Log,
            scroll
        } if scroll == max
    ));
    app.scroll_help(5);
    assert!(matches!(
        app.help_view,
        crate::help::HelpView::Page { scroll, .. } if scroll == max
    ));
}

#[test]
fn test_time_bound_alone_activates_filter_and_matches() {
    let mut app = App::new(100);
    assert!(!app.filter_active());
    app.time_bound = Some(TimeBound {
        since: Some("10:00:00".into()),
        until: Some("10:00:00".into()),
    });
    app.rebuild_visible();
    assert!(app.filter_active());
    let keep = row("A");
    let late = EntryRow::from_line("04-02 11:00:00.000  1  1 I B   : m").unwrap();
    assert!(app.row_passes_filters(&keep));
    assert!(!app.row_passes_filters(&late));
    let (tx, rx) = mpsc::channel();
    tx.send(keep).unwrap();
    tx.send(late).unwrap();
    drop(tx);
    app.drain(&rx);
    assert_eq!(app.index.visible.len(), 1);
    assert_eq!(
        app.view_source()[app.source_idx_for_visible(0).unwrap()].tag,
        "A"
    );
    assert_eq!(app.time_badge_label().as_deref(), Some("10:00:00…10:00:00"));
    assert!(app
        .export_cli_command()
        .contains(r#"time >= "10:00:00" and time <= "10:00:00""#));
}

#[test]
fn test_clear_time_bound_restores_visibility() {
    let mut app = App::new(100);
    app.time_bound = Some(TimeBound {
        since: Some("11:00:00".into()),
        until: None,
    });
    app.rebuild_visible();
    let (tx, rx) = mpsc::channel();
    tx.send(row("A")).unwrap();
    drop(tx);
    app.drain(&rx);
    assert!(app.index.visible.is_empty());
    app.clear_time_bound();
    assert!(app.time_bound.is_none());
    assert!(!app.filter_active());
    assert_eq!(app.index.visible.len(), 1);
}

#[test]
fn test_time_badge_label() {
    let mut app = App::new(100);
    assert!(app.time_badge_label().is_none());
    app.time_bound = Some(TimeBound {
        since: Some("04-02 10:00:00".into()),
        until: None,
    });
    assert_eq!(app.time_badge_label().as_deref(), Some("≥04-02 10:00:00"));
}

#[test]
fn test_view_focus_highlight_and_severe_toggle() {
    use crate::highlight_model::HighlightGroup;
    let mut app = App::new(100);
    let (tx, rx) = mpsc::channel();
    tx.send(row_level_msg('I', "T", "hit one")).unwrap();
    tx.send(row_level_msg('E', "T", "hit err")).unwrap();
    tx.send(row_level_msg('E', "T", "plain err")).unwrap();
    tx.send(row_level_msg('I', "T", "other")).unwrap();
    drop(tx);
    app.drain(&rx);
    app.push_or_find_highlight_group(HighlightGroup::from_pattern("hit").unwrap());
    app.following = true;

    app.toggle_view_focus(ViewFocusKind::Highlight);
    assert!(app.view_focus.highlight);
    assert!(!app.view_focus.severe);
    assert!(!app.following);
    assert!(app.filter_active());
    assert_eq!(app.index.visible.len(), 2);
    assert_eq!(app.view_focus_badge_label(), Some("HL"));

    // Independent: fe stacks with fh → intersection.
    app.toggle_view_focus(ViewFocusKind::Severe);
    assert!(app.view_focus.highlight);
    assert!(app.view_focus.severe);
    assert_eq!(app.index.visible.len(), 1);
    assert_eq!(app.current_row().unwrap().msg, "hit err");
    assert_eq!(app.view_focus_badge_label(), Some("HL+ERR"));

    app.following = true;
    app.resume_following();
    assert!(app.following);
    assert!(
        app.view_focus.highlight && app.view_focus.severe,
        "Esc resume keeps both focus bits"
    );

    app.toggle_view_focus(ViewFocusKind::Highlight);
    assert!(!app.view_focus.highlight);
    assert!(app.view_focus.severe);
    assert_eq!(app.index.visible.len(), 2, "fe-only keeps both severe rows");
    assert_eq!(app.view_focus_badge_label(), Some("ERR"));

    app.toggle_view_focus(ViewFocusKind::Severe);
    assert!(!app.view_focus.is_active());
    assert_eq!(app.index.visible.len(), 4);
}

#[test]
fn test_view_focus_fh_without_highlight_flashes() {
    let mut app = App::new(100);
    let (tx, rx) = mpsc::channel();
    tx.send(row("A")).unwrap();
    drop(tx);
    app.drain(&rx);
    app.toggle_view_focus(ViewFocusKind::Highlight);
    assert!(!app.view_focus.is_active());
    assert_eq!(app.status_msg.as_deref(), Some("NO HIGHLIGHT"));
    assert_eq!(app.index.visible.len(), 1);
}

#[test]
fn test_view_focus_fh_ands_after_chip_filter() {
    use crate::highlight_model::HighlightGroup;
    use crate::input::{build_group_from_chips, Chip, ChipField};

    let mut app = App::new(100);
    let (tx, rx) = mpsc::channel();
    tx.send(row_level_msg('I', "Keep", "hit alpha")).unwrap();
    tx.send(row_level_msg('I', "Keep", "plain")).unwrap();
    tx.send(row_level_msg('I', "Drop", "hit beta")).unwrap();
    drop(tx);
    app.drain(&rx);

    app.groups.groups.push(
        build_group_from_chips(
            vec![Chip {
                field: ChipField::Tag,
                value: "Keep".into(),
            }],
            true,
        )
        .unwrap()
        .unwrap(),
    );
    app.push_or_find_highlight_group(HighlightGroup::from_pattern("hit").unwrap());
    app.rebuild_visible();
    assert_eq!(
        app.index.visible.len(),
        2,
        "chip filter keeps both Keep rows"
    );

    app.toggle_view_focus(ViewFocusKind::Highlight);
    assert!(app.view_focus.highlight);
    assert_eq!(
        app.index.visible.len(),
        1,
        "fh keeps only highlight hits inside filter"
    );
    assert_eq!(app.current_row().unwrap().msg, "hit alpha");

    app.toggle_view_focus(ViewFocusKind::Highlight);
    assert!(!app.view_focus.is_active());
    assert_eq!(
        app.index.visible.len(),
        2,
        "second fh restores chip-filter-only view"
    );
}

fn row_level_msg(level: char, tag: &str, msg: &str) -> EntryRow {
    EntryRow::from_line(&format!(
        "04-02 10:00:00.000  1234  5678 {level} {tag}   : {msg}"
    ))
    .unwrap()
}

#[test]
fn test_di_group_zero_does_not_clear_time_bound() {
    use crate::input::{Chip, ChipField};

    let mut app = App::new(100);
    app.time_bound = Some(TimeBound {
        since: Some("10:30:00".into()),
        until: None,
    });
    app.groups.groups.push(Group {
        label: "tag:A".into(),
        chips: vec![Chip {
            field: ChipField::Tag,
            value: "A".into(),
        }],
        enabled: true,
        same_field_op: crate::fuzzy::SameFieldOp::And,
    });
    app.rebuild_visible();
    let (tx, rx) = mpsc::channel();
    tx.send(row("A")).unwrap(); // 10:00:00 — excluded by since
    tx.send(EntryRow::from_line("04-02 11:00:00.000  1  1 I A   : late").unwrap())
        .unwrap();
    drop(tx);
    app.drain(&rx);
    assert_eq!(app.index.visible.len(), 1);
    assert_eq!(
        app.view_source()[app.source_idx_for_visible(0).unwrap()]
            .as_log_entry()
            .time_hms(),
        Some("11:00:00")
    );

    app.groups.groups[0].enabled = false;
    app.rebuild_visible();
    // Group disabled → include vacuous, but global time window still AND.
    assert!(app.time_bound.is_some());
    assert!(app.filter_active());
    assert_eq!(app.index.visible.len(), 1);
    assert_eq!(
        app.view_source()[app.source_idx_for_visible(0).unwrap()]
            .as_log_entry()
            .time_hms(),
        Some("11:00:00")
    );
}

#[test]
fn test_ring_buffer_evicts_oldest_and_shifts_indices() {
    let mut app = App::new(2);
    let (tx, rx) = mpsc::channel();
    tx.send(row("A")).unwrap();
    tx.send(row("B")).unwrap();
    tx.send(row("C")).unwrap();
    drop(tx);
    app.drain(&rx);
    assert_eq!(app.rows().len(), 2);
    assert_eq!(app.rows()[0].tag, "B");
    assert_eq!(app.rows()[1].tag, "C");
    assert_eq!(app.index.visible, Visible::All { len: 2 });
}

#[test]
fn test_visible_all_identity_under_filter_active() {
    let mut app = App::new(100);
    app.groups = GroupList {
        groups: vec![filter_group("keep-A", "A")],
        excludes: Vec::new(),
    };
    app.rebuild_visible();
    let (tx, rx) = mpsc::channel();
    tx.send(row("A")).unwrap();
    tx.send(row("B")).unwrap();
    tx.send(row("A")).unwrap();
    drop(tx);
    app.drain(&rx);
    assert_eq!(app.matched().len(), 2);
    assert_eq!(app.index.visible, Visible::All { len: 2 });
    assert_eq!(app.source_idx_for_visible(0), Some(0));
    assert_eq!(app.source_idx_for_visible(1), Some(1));
    assert_eq!(app.source_idx_for_visible(2), None);
}

#[test]
fn test_drain_ring_applies_push_row() {
    use crate::ingest::DropOldestRing;
    let mut app = App::new(100);
    let ring = DropOldestRing::new(8);
    ring.push(row("A"));
    ring.push(row("B"));
    ring.mark_disconnected();
    app.drain(&ring);
    assert_eq!(app.index.visible.len(), 2);
    assert!(app.ingest_done);
    assert_eq!(app.rows()[0].tag, "A");
    assert_eq!(app.rows()[1].tag, "B");
}

#[test]
fn test_drain_budget_leaves_remainder_and_defers_ingest_done() {
    use crate::ingest::DropOldestRing;
    let mut app = App::new(DRAIN_BUDGET_PER_FRAME + 8);
    let ring = DropOldestRing::new(DRAIN_BUDGET_PER_FRAME + 4);
    for i in 0..(DRAIN_BUDGET_PER_FRAME + 3) {
        ring.push(row(&format!("T{i}")));
    }
    ring.mark_disconnected();
    app.drain(&ring);
    assert_eq!(app.rows().len(), DRAIN_BUDGET_PER_FRAME);
    assert!(
        !app.ingest_done,
        "budget stop must not mark done while rows remain"
    );
    assert_eq!(ring.len(), 3);
    app.drain(&ring);
    assert_eq!(app.rows().len(), DRAIN_BUDGET_PER_FRAME + 3);
    assert!(app.ingest_done);
    assert!(ring.is_empty());
}

#[test]
fn test_inactive_front_evict_decrements_list_offset_o1() {
    let mut app = App::new(2);
    let (tx, rx) = mpsc::channel();
    tx.send(row("A")).unwrap();
    tx.send(row("B")).unwrap();
    drop(tx);
    app.drain(&rx);
    app.following = false;
    app.list_offset = 1;
    app.cursor = 1;
    assert_eq!(app.index.visible, Visible::All { len: 2 });

    let (tx2, rx2) = mpsc::channel();
    tx2.send(row("C")).unwrap(); // rows at cap → pop A, O(1) adjust
    drop(tx2);
    app.drain(&rx2);

    assert_eq!(app.rows()[0].tag, "B");
    assert_eq!(app.rows()[1].tag, "C");
    assert_eq!(app.index.visible, Visible::All { len: 2 });
    assert_eq!(app.list_offset, 0);
    assert_eq!(app.cursor, 0);
}

#[test]
fn test_evicting_visible_front_decrements_list_offset() {
    // With the matched buffer, a filter-matching row is NOT evicted by
    // `rows` overflow — only by reaching `matched_cap`. Simulate that:
    // two matching rows fill `matched`; a third match evicts the oldest.
    let mut app = App::new(100);
    app.index.matched_cap = 2;
    app.groups = GroupList {
        groups: vec![filter_group("keep-A", "A")],
        excludes: Vec::new(),
    };
    app.rebuild_visible();
    let (tx, rx) = mpsc::channel();
    tx.send(row("A")).unwrap(); // matched=[A0], visible=[0]
    tx.send(row("A")).unwrap(); // matched=[A0,A1], visible=[0,1]
    drop(tx);
    app.drain(&rx);
    assert_eq!(app.index.visible.len(), 2);
    app.following = false;
    app.list_offset = 1;
    app.cursor = 1;

    let (tx2, rx2) = mpsc::channel();
    tx2.send(row("A")).unwrap(); // matched at cap → evict A0 → visible front drops
    drop(tx2);
    app.drain(&rx2);

    assert_eq!(app.index.visible.len(), 2);
    assert_eq!(
        app.list_offset, 0,
        "viewport must shift with front eviction"
    );
    assert_eq!(app.cursor, 0);
}

#[test]
fn test_move_cursor_clamps() {
    let mut app = App::new(100);
    let (tx, rx) = mpsc::channel();
    tx.send(row("A")).unwrap();
    tx.send(row("B")).unwrap();
    drop(tx);
    app.drain(&rx);
    app.move_cursor(-5);
    assert_eq!(app.cursor, 0);
    app.move_cursor(5);
    assert_eq!(app.cursor, 1);
}

#[test]
fn test_move_cursor_manual_large_delta_clamps_like_paging() {
    let mut app = App::new(100);
    let (tx, rx) = mpsc::channel();
    tx.send(row("A")).unwrap();
    tx.send(row("B")).unwrap();
    drop(tx);
    app.drain(&rx);
    app.move_cursor_manual(-10); // simulates Ctrl-u paging past the top
    assert_eq!(app.cursor, 0);
    assert!(!app.following, "negative delta should pause following");
    app.move_cursor_manual(10); // simulates Ctrl-d paging past the bottom
    assert_eq!(app.cursor, 1);
    assert!(
        app.following,
        "landing on bottom via large positive delta resumes following"
    );
}

#[test]
fn clear_buffered_logs_drops_buffers_keeps_filters_and_resumes() {
    use crate::bookmark::Bookmark;
    use crate::highlight_model::HighlightGroup;

    let mut app = App::new(100);
    app.groups = GroupList {
        groups: vec![filter_group("keep-A", "A")],
        excludes: Vec::new(),
    };
    app.push_or_find_highlight_group(HighlightGroup::from_pattern("err").unwrap());
    app.lock_pid = Some("1".into());
    app.rebuild_visible();

    let (tx, rx) = mpsc::channel();
    tx.send(row("A")).unwrap();
    tx.send(row("B")).unwrap();
    drop(tx);
    app.drain(&rx);
    assert!(!app.rows().is_empty());
    assert!(!app.matched().is_empty());
    assert!(!app.index.visible.is_empty());

    let row_id = app.matched()[0].row_id;
    let mut snap = app.matched()[0].clone();
    snap.row_id = row_id;
    app.bookmarks.try_add(Bookmark::from_row(snap)).unwrap();
    app.bookmark_row_ids.insert(row_id);

    app.following = false;
    app.cursor = 0;
    app.list_offset = 0;
    app.pending = Pending::Chip;
    app.visual_anchor = Some(0);

    app.clear_buffered_logs();

    assert!(app.rows().is_empty());
    assert!(app.matched().is_empty());
    assert!(app.index.visible.is_empty());
    assert!(app.bookmarks.is_empty());
    assert!(app.bookmark_row_ids.is_empty());
    assert_eq!(app.groups.groups.len(), 1);
    assert_eq!(app.highlight_groups.groups.len(), 1);
    assert_eq!(app.lock_pid.as_deref(), Some("1"));
    assert!(app.following);
    assert_eq!(app.cursor, 0);
    assert_eq!(app.list_offset, 0);
    assert_ne!(app.pending, Pending::Yank);
    assert_ne!(app.pending, Pending::Chip);
    assert!(app.visual_anchor.is_none());
    assert_eq!(app.status_msg.as_deref(), Some("CLEARED"));
}

#[test]
fn reset_for_source_switch_keeps_only_filter_exclude_highlight() {
    use crate::highlight_model::HighlightGroup;
    use crate::input::{Chip, ChipField};

    let mut app = App::new(100);
    app.groups = GroupList {
        groups: vec![filter_group("keep", "A")],
        excludes: vec![crate::filter_model::ExcludeEntry {
            chip: Chip {
                field: ChipField::Tag,
                value: "noise".into(),
            },
            enabled: true,
        }],
    };
    app.push_or_find_highlight_group(HighlightGroup::from_pattern("err").unwrap());
    app.lock_pid = Some("9".into());
    app.time_bound = Some(TimeBound {
        since: Some("01-01 00:00:00.000".into()),
        until: None,
    });
    app.rebuild_visible();
    let (tx, rx) = mpsc::channel();
    tx.send(row("A")).unwrap();
    drop(tx);
    app.drain(&rx);

    app.reset_for_source_switch();

    assert!(app.rows().is_empty());
    assert_eq!(app.groups.groups.len(), 1);
    assert_eq!(app.groups.excludes.len(), 1);
    assert_eq!(app.highlight_groups.groups.len(), 1);
    assert!(app.lock_pid.is_none());
    assert!(app.time_bound.is_none());
    assert!(app.following);
}

#[test]
fn test_cursor_unaffected_when_evicted_row_was_already_filtered_out() {
    let mut app = App::new(3);
    app.groups = GroupList {
        groups: vec![filter_group("x", "X")],
        excludes: Vec::new(),
    };
    app.rebuild_visible();
    let (tx, rx) = std::sync::mpsc::channel();
    tx.send(row("N1")).unwrap(); // filtered out, not in `matched`/`visible`
    tx.send(row("X1")).unwrap();
    tx.send(row("X2")).unwrap();
    drop(tx);
    app.drain(&rx);
    assert_eq!(app.index.visible, Visible::All { len: 2 });
    app.cursor = 1; // pointing at X2
    app.following = false;
    let selected_tag_before = app.current_row().unwrap().tag.clone();

    let (tx2, rx2) = std::sync::mpsc::channel();
    tx2.send(row("X3")).unwrap(); // triggers `rows` eviction of N1 (non-matching)
    drop(tx2);
    app.drain(&rx2);

    let selected_tag_after = app.current_row().unwrap().tag.clone();
    assert_eq!(
        selected_tag_before, selected_tag_after,
        "cursor should still point at the same logical row"
    );
    assert_eq!(selected_tag_after, "X2");
}

#[test]
fn test_matched_rows_survive_rows_overflow() {
    // The core fix: with a filter active, matching rows are retained in
    // `matched` even after `rows` rolls over. Non-matching churn must not
    // wash out previously matched rows.
    let mut app = App::new(2);
    app.groups = GroupList {
        groups: vec![filter_group("keep-A", "A")],
        excludes: Vec::new(),
    };
    app.rebuild_visible();
    let (tx, rx) = mpsc::channel();
    tx.send(row("A")).unwrap(); // A0: matched=[A0], rows=[A0]
    tx.send(row("X")).unwrap(); // rows=[A0,X], matched=[A0]
    tx.send(row("Y")).unwrap(); // rows rolls: [X,Y], A0 evicted from rows
    drop(tx);
    app.drain(&rx);
    assert_eq!(app.rows().len(), 2);
    assert_eq!(app.rows()[0].tag, "X");
    assert_eq!(app.rows()[1].tag, "Y");
    assert_eq!(app.matched().len(), 1);
    assert_eq!(app.matched()[0].tag, "A");
    assert_eq!(app.index.visible, Visible::All { len: 1 });
    assert_eq!(app.current_row().unwrap().tag, "A");
}

#[test]
fn test_rebuild_visible_after_filter_change_loses_matched_evicted_from_rows() {
    // Rebuild re-scans current `rows`; rows already evicted from `rows`
    // (even if previously retained in `matched`) are unrecoverable.
    let mut app = App::new(2);
    // Start with NO filter: everything goes to `rows`, `matched` stays empty.
    let (tx, rx) = mpsc::channel();
    tx.send(row("A")).unwrap();
    tx.send(row("B")).unwrap();
    tx.send(row("A")).unwrap(); // rows rolls: [B,A]
    drop(tx);
    app.drain(&rx);
    assert_eq!(app.rows()[0].tag, "B");
    assert_eq!(app.rows()[1].tag, "A");
    // Now activate a filter matching `A`; rebuild scans only current rows
    // ([B,A]) — the first `A` (already evicted from `rows`) is gone.
    app.groups = GroupList {
        groups: vec![filter_group("keep-A", "A")],
        excludes: Vec::new(),
    };
    app.rebuild_visible();
    assert_eq!(app.matched().len(), 1);
    assert_eq!(app.matched()[0].tag, "A");
    assert_eq!(app.index.visible, Visible::All { len: 1 });
}

#[test]
fn summary_panel_stream_ready_reflects_visible() {
    let mut app = App::new(100);
    let (tx, rx) = mpsc::channel();
    tx.send(EntryRow::from_line("04-02 10:00:00.000  1  1 I TagA   : ok").unwrap())
        .unwrap();
    tx.send(EntryRow::from_line("04-02 10:00:01.000  1  1 E TagA   : boom").unwrap())
        .unwrap();
    tx.send(
        EntryRow::from_line("04-02 10:00:02.000  1  1 E AndroidRuntime: FATAL EXCEPTION: main")
            .unwrap(),
    )
    .unwrap();
    drop(tx);
    app.drain(&rx);
    assert_eq!(app.index.visible.len(), 3);

    assert!(!app.summary_open());
    app.open_summary_panel();
    assert!(app.summary_open());
    assert!(matches!(app.summary_view, SummaryView::Loading));
    app.flush_summary_job(Duration::from_secs(5));
    let SummaryView::Ready(report) = &app.summary_view else {
        panic!("expected Ready after flush");
    };
    assert_eq!(report.total, 3);
    assert_eq!(report.matched, 3);
    assert_eq!(report.crashes, 1);
    assert_eq!(*report.levels.get(&'E').unwrap_or(&0), 2);
    assert_eq!(*report.levels.get(&'I').unwrap_or(&0), 1);
    assert!(report
        .top_tags
        .iter()
        .any(|t| t.tag == "TagA" && t.count == 2));

    app.close_summary_panel();
    assert!(!app.summary_open());
}

#[test]
fn hist_panel_jumps_to_severe_and_applies_window() {
    let mut app = App::new(100);
    app.export_source = crate::export::ExportSource::File("demo.log".into());
    let (tx, rx) = mpsc::channel();
    tx.send(EntryRow::from_line("04-02 10:32:05.000  1  1 I Tag     : ok").unwrap())
        .unwrap();
    tx.send(EntryRow::from_line("04-02 10:32:30.000  1  1 E Tag     : boom").unwrap())
        .unwrap();
    tx.send(EntryRow::from_line("04-02 10:33:05.000  1  1 I Tag     : later").unwrap())
        .unwrap();
    drop(tx);
    app.drain(&rx);
    app.following = true;
    assert!(app.open_hist_panel());
    assert!(!app.following);
    app.flush_hist_job(std::time::Duration::from_secs(2));
    assert!(app.hist_open());
    let crate::hist_panel::HistView::Ready(report) = &app.hist_view else {
        panic!("expected ready hist");
    };
    assert!(report.buckets.len() >= 2);
    let severe_i = report
        .buckets
        .iter()
        .position(|b| b.severe > 0)
        .expect("severe bucket");
    app.hist_cursor = severe_i;
    app.submit_hist_jump();
    assert!(!app.hist_open());
    assert_eq!(app.cursor, 1, "severe bucket jumps to its E row");

    assert!(app.open_hist_panel());
    app.flush_hist_job(std::time::Duration::from_secs(2));
    let crate::hist_panel::HistView::Ready(report) = &app.hist_view else {
        panic!("expected ready hist");
    };
    app.hist_cursor = report
        .buckets
        .iter()
        .position(|b| b.severe > 0)
        .expect("severe bucket");
    app.apply_hist_window();
    assert!(!app.hist_open());
    assert!(app.time_bound.as_ref().is_some_and(|t| t.is_active()));
    assert_eq!(
        app.index.visible.len(),
        1,
        "severe bucket window keeps the E row"
    );
}

#[test]
fn hist_panel_no_dates_flashes() {
    let mut app = App::new(100);
    app.export_source = crate::export::ExportSource::File("demo.log".into());
    assert!(!app.open_hist_panel());
    assert_eq!(app.status_msg.as_deref(), Some("NO DATES"));
    assert!(!app.hist_open());
}

#[test]
fn summary_panel_rapid_reopen_drops_stale_gen_result() {
    let mut app = App::new(100);
    let (tx, rx) = mpsc::channel();
    tx.send(row("A")).unwrap();
    drop(tx);
    app.drain(&rx);

    app.open_summary_panel();
    let stale_ticket = app.summary_ticket();
    app.close_summary_panel();
    app.open_summary_panel();

    // Simulate a slow background result from the *first* (now-closed) request
    // arriving after the second request was opened.
    let stale_report = alnav::summary::SummaryOutput {
        total: 999,
        matched: 999,
        levels: std::collections::HashMap::new(),
        top_tags: Vec::new(),
        time_range: alnav::summary::TimeRange {
            first: String::new(),
            last: String::new(),
        },
        top_errors: Vec::new(),
        crashes: 0,
    };
    stale_ticket.send(stale_report);
    app.poll_summary_job();
    assert!(
        matches!(app.summary_view, SummaryView::Loading),
        "stale gen result must not overwrite Loading"
    );

    app.flush_summary_job(Duration::from_secs(5));
    let SummaryView::Ready(report) = &app.summary_view else {
        panic!("expected Ready from the fresh request");
    };
    assert_eq!(report.total, 1);
    assert_ne!(report.total, 999);
}
