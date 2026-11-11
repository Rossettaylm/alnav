//! Highlight box, time/hist, compare, bookmark and preset panel tests.

use super::*;
use crate::app::App;
use crate::app::Pending;
use crate::handlers::handle_normal_event;
use crate::handlers::normal::handle_normal_key;
use crate::handlers::panels::{
    handle_compare_key, handle_highlight_box_key, handle_hist_panel_key, handle_preset_name_key,
    handle_time_panel_key,
};
use crate::handlers::picker::keys::handle_picker_key;
use crate::handlers::picker::render_data::picker_render_data;
use crate::{app, export, highlight_model, input, model};
use crossterm::event::{self, KeyCode};
use crossterm::event::{KeyEvent, KeyModifiers};

#[test]
fn test_highlight_box_tab_confirms_selected_candidate() {
    let mut app = App::new(100);
    app.highlight_groups
        .groups
        .push(highlight_model::HighlightGroup::from_pattern("error").unwrap());
    app.highlight_groups
        .groups
        .push(highlight_model::HighlightGroup::from_pattern("errno").unwrap());
    app.highlight_box.begin_editing();
    for c in "er".chars() {
        app.highlight_box.push_char(c);
    }
    handle_highlight_box_key(&mut app, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    handle_highlight_box_key(&mut app, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert!(!app.highlight_box.editing);
    assert_eq!(
        app.highlight_groups.groups.len(),
        2,
        "must reuse existing, not add"
    );
    assert_eq!(app.focus, app::Focus::LogList);
}

#[test]
fn test_highlight_box_enter_creates_when_no_candidate_match() {
    let mut app = App::new(100);
    app.highlight_groups
        .groups
        .push(highlight_model::HighlightGroup::from_pattern("error").unwrap());
    app.highlight_box.begin_editing();
    for c in "unique".chars() {
        app.highlight_box.push_char(c);
    }
    handle_highlight_box_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(app.highlight_groups.groups.len(), 2);
    assert_eq!(app.highlight_groups.groups[1].pattern, "unique");
}

#[test]
fn test_highlight_box_enter_adds_group_ignore_case() {
    let mut app = App::new(100);
    app.highlight_box.editing = true;
    for c in "ERROR".chars() {
        app.highlight_box.push_char(c);
    }
    handle_highlight_box_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(!app.highlight_box.editing);
    assert_eq!(app.highlight_groups.groups.len(), 1);
    assert!(app.highlight_groups.any_match("", "an error occurred"));
}

#[test]
fn test_highlight_box_enter_jumps_to_first_match() {
    let mut app = App::new(100);
    let (tx, rx) = std::sync::mpsc::channel();
    tx.send(model::EntryRow::from_line("04-02 10:00:00.000  1  1 I T   : aaa").unwrap())
        .unwrap();
    tx.send(model::EntryRow::from_line("04-02 10:00:01.000  1  1 I T   : findme here").unwrap())
        .unwrap();
    tx.send(model::EntryRow::from_line("04-02 10:00:02.000  1  1 I T   : findme two").unwrap())
        .unwrap();
    drop(tx);
    app.drain(&rx);
    app.following = true;
    app.cursor = 2;

    app.highlight_box.editing = true;
    for c in "findme".chars() {
        app.highlight_box.push_char(c);
    }
    handle_highlight_box_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(app.cursor, 1);
    assert!(!app.following);
    assert_eq!(app.focus, app::Focus::LogList);
}

#[test]
fn test_highlight_box_space_is_literal_single_enter_submits() {
    let mut app = App::new(100);
    app.highlight_box.editing = true;
    for c in "foo bar".chars() {
        app.highlight_box.push_char(c);
    }
    handle_highlight_box_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(app.highlight_groups.groups.len(), 1);
    assert_eq!(app.highlight_groups.groups[0].pattern, "foo bar");
    assert_eq!(app.highlight_groups.active_patterns().len(), 1);
}

#[test]
fn test_highlight_box_duplicate_jumps_without_adding() {
    let mut app = App::new(100);
    let (tx, rx) = std::sync::mpsc::channel();
    tx.send(model::EntryRow::from_line("04-02 10:00:00.000  1  1 I T   : aaa").unwrap())
        .unwrap();
    tx.send(model::EntryRow::from_line("04-02 10:00:01.000  1  1 I T   : findme here").unwrap())
        .unwrap();
    drop(tx);
    app.drain(&rx);
    app.following = false;
    app.cursor = 0;

    app.highlight_box.editing = true;
    for c in "findme".chars() {
        app.highlight_box.push_char(c);
    }
    handle_highlight_box_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(app.highlight_groups.groups.len(), 1);
    assert_eq!(app.cursor, 1);

    app.cursor = 0;
    app.highlight_box.editing = true;
    for c in "FINDME".chars() {
        app.highlight_box.push_char(c);
    }
    handle_highlight_box_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(
        app.highlight_groups.groups.len(),
        1,
        "duplicate must not add another group"
    );
    assert_eq!(app.cursor, 1, "duplicate still jumps to first match");
}

#[test]
fn test_highlight_box_ctrl_c_cancels_without_adding() {
    let mut app = App::new(100);
    app.highlight_box.editing = true;
    app.highlight_box.push_char('x');
    handle_highlight_box_key(
        &mut app,
        KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
    );
    assert!(!app.highlight_box.editing);
    assert!(app.highlight_groups.groups.is_empty());
}

#[test]
fn test_highlight_box_literal_metacharacters_do_not_drop_prior_groups() {
    // Patterns are literal (regex-escaped); metacharacters are valid input.
    let mut app = App::new(100);
    app.highlight_groups
        .groups
        .push(highlight_model::HighlightGroup::from_pattern("existing").unwrap());
    app.highlight_box.editing = true;
    for c in "(unclosed".chars() {
        app.highlight_box.push_char(c);
    }
    handle_highlight_box_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(!app.highlight_box.editing);
    assert_eq!(app.highlight_groups.groups.len(), 2);
    assert!(app.highlight_groups.any_match("", "existing"));
    assert!(app.highlight_groups.any_match("", "see (unclosed here"));
}

#[test]
fn test_highlight_box_esc_and_enter_return_focus_to_loglist() {
    let mut app = App::new(100);
    app.focus = app::Focus::Input;
    app.highlight_box.editing = true;
    app.highlight_box.push_char('x');
    handle_highlight_box_key(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(app.focus, app::Focus::LogList);

    app.focus = app::Focus::ChipStrip;
    app.highlight_box.editing = true;
    app.highlight_box.push_char('y');
    handle_highlight_box_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(app.focus, app::Focus::LogList);
}

#[test]
fn test_tt_opens_panel_in_file_mode_and_tu_clears() {
    let mut app = App::new(100);
    let mut input = input::InputBox::default();
    app.export_source = export::ExportSource::File("demo.log".into());
    drain_lines(
        &mut app,
        &[
            "04-02 10:00:00.000  1  1 I Tag     : a",
            "04-02 12:00:00.000  1  1 I Tag     : b",
        ],
    );
    app.following = true;
    handle_normal_key(&mut app, &mut input, KeyCode::Char('t'));
    assert_eq!(app.pending, Pending::Time);
    handle_normal_key(&mut app, &mut input, KeyCode::Char('t'));
    assert!(app.time_panel.is_some());
    assert!(!app.following, "tt opens with following=false");
    assert_ne!(app.pending, Pending::Time);

    // Submit a one-sided since window via panel API (key path covered above).
    app.apply_time_bound(crate::filter_model::TimeBound {
        since: Some("04-02 11:00:00".into()),
        until: None,
    });
    assert!(app.filter_active());
    assert_eq!(app.visible.len(), 1);
    assert!(app.export_cli_command().contains("--since "));

    app.following = true;
    handle_normal_key(&mut app, &mut input, KeyCode::Char('t'));
    handle_normal_key(&mut app, &mut input, KeyCode::Char('u'));
    assert!(app.time_bound.is_none());
    assert!(!app.following);
    assert_eq!(app.visible.len(), 2);
    assert_eq!(app.status_msg.as_deref(), Some("TIME CLEARED"));
}

#[test]
fn test_th_opens_hist_enter_jumps_esc_closes() {
    let mut app = App::new(100);
    let mut input = input::InputBox::default();
    app.export_source = export::ExportSource::File("demo.log".into());
    drain_lines(
        &mut app,
        &[
            "04-02 10:32:05.000  1  1 I Tag     : ok",
            "04-02 10:32:30.000  1  1 E Tag     : boom",
            "04-02 10:33:05.000  1  1 I Tag     : later",
        ],
    );
    app.following = true;
    handle_normal_key(&mut app, &mut input, KeyCode::Char('t'));
    handle_normal_key(&mut app, &mut input, KeyCode::Char('h'));
    assert!(app.hist_open());
    assert!(!app.following);
    app.flush_hist_job(std::time::Duration::from_secs(2));
    if let crate::hist_panel::HistView::Ready(report) = &app.hist_view {
        app.hist_cursor = report
            .buckets
            .iter()
            .position(|b| b.severe > 0)
            .expect("severe bucket");
    }
    handle_hist_panel_key(
        &mut app,
        event::KeyEvent::new(KeyCode::Enter, event::KeyModifiers::NONE),
    );
    assert!(!app.hist_open());
    assert_eq!(app.cursor, 1);

    handle_normal_key(&mut app, &mut input, KeyCode::Char('t'));
    handle_normal_key(&mut app, &mut input, KeyCode::Char('h'));
    app.flush_hist_job(std::time::Duration::from_secs(2));
    let following = app.following;
    handle_hist_panel_key(
        &mut app,
        event::KeyEvent::new(KeyCode::Esc, event::KeyModifiers::NONE),
    );
    assert!(!app.hist_open());
    assert_eq!(app.following, following, "Esc does not resume following");
}

#[test]
fn test_hist_panel_fast_move_and_tab_cycles_interval() {
    let mut app = App::new(100);
    let mut input = input::InputBox::default();
    app.export_source = export::ExportSource::File("demo.log".into());
    let owned: Vec<String> = (0..20)
        .map(|i| {
            let total = i * 10;
            format!(
                "04-02 10:{:02}:{:02}.000  1  1 I Tag     : n{i}",
                total / 60,
                total % 60
            )
        })
        .collect();
    let lines: Vec<&str> = owned.iter().map(String::as_str).collect();
    drain_lines(&mut app, &lines);

    handle_normal_key(&mut app, &mut input, KeyCode::Char('t'));
    handle_normal_key(&mut app, &mut input, KeyCode::Char('h'));
    app.flush_hist_job(std::time::Duration::from_secs(2));
    let n = match &app.hist_view {
        crate::hist_panel::HistView::Ready(r) => {
            assert_eq!(r.interval_secs, 10);
            assert!(r.buckets.len() >= 15, "need enough buckets for J/K");
            r.buckets.len()
        }
        crate::hist_panel::HistView::Loading { .. } => panic!("expected Ready, still Loading"),
        crate::hist_panel::HistView::Closed => panic!("expected Ready, got Closed"),
    };
    assert_eq!(app.hist_cursor, 0);

    handle_hist_panel_key(
        &mut app,
        event::KeyEvent::new(KeyCode::Char('J'), event::KeyModifiers::SHIFT),
    );
    assert_eq!(app.hist_cursor, crate::help::FAST_SCROLL_STEP as usize);
    handle_hist_panel_key(
        &mut app,
        event::KeyEvent::new(KeyCode::Char('K'), event::KeyModifiers::SHIFT),
    );
    assert_eq!(app.hist_cursor, 0);
    handle_hist_panel_key(
        &mut app,
        event::KeyEvent::new(KeyCode::Char('G'), event::KeyModifiers::SHIFT),
    );
    assert_eq!(app.hist_cursor, n - 1);
    handle_hist_panel_key(
        &mut app,
        event::KeyEvent::new(KeyCode::Char('g'), event::KeyModifiers::NONE),
    );
    assert_eq!(app.hist_cursor, 0);

    let interval = |app: &App| match &app.hist_view {
        crate::hist_panel::HistView::Ready(r) => r.interval_secs,
        crate::hist_panel::HistView::Loading { interval_secs } => *interval_secs,
        crate::hist_panel::HistView::Closed => panic!("hist closed"),
    };

    handle_hist_panel_key(
        &mut app,
        event::KeyEvent::new(KeyCode::Char('z'), event::KeyModifiers::NONE),
    );
    assert_eq!(interval(&app), 10, "z must no longer change interval");

    handle_hist_panel_key(
        &mut app,
        event::KeyEvent::new(KeyCode::Tab, event::KeyModifiers::NONE),
    );
    app.flush_hist_job(std::time::Duration::from_secs(2));
    assert_eq!(interval(&app), 60);
    handle_hist_panel_key(
        &mut app,
        event::KeyEvent::new(KeyCode::Tab, event::KeyModifiers::NONE),
    );
    app.flush_hist_job(std::time::Duration::from_secs(2));
    assert_eq!(interval(&app), 300);
    handle_hist_panel_key(
        &mut app,
        event::KeyEvent::new(KeyCode::Tab, event::KeyModifiers::NONE),
    );
    app.flush_hist_job(std::time::Duration::from_secs(2));
    assert_eq!(interval(&app), 10);
    handle_hist_panel_key(
        &mut app,
        event::KeyEvent::new(KeyCode::BackTab, event::KeyModifiers::SHIFT),
    );
    app.flush_hist_job(std::time::Duration::from_secs(2));
    assert_eq!(interval(&app), 300);
}

#[test]
fn test_tt_empty_candidates_flashes_and_live_modes_hide_t() {
    let mut app = App::new(100);
    let mut input = input::InputBox::default();
    app.export_source = export::ExportSource::File("demo.log".into());
    // No rows → no date candidates.
    handle_normal_key(&mut app, &mut input, KeyCode::Char('t'));
    handle_normal_key(&mut app, &mut input, KeyCode::Char('t'));
    assert!(app.time_panel.is_none());
    assert_eq!(app.status_msg.as_deref(), Some("NO DATES"));

    // Abandoned `ts` → UNKNOWN, panel stays closed.
    app.status_msg = None;
    drain_lines(&mut app, &["04-02 10:00:00.000  1  1 I Tag     : a"]);
    handle_normal_key(&mut app, &mut input, KeyCode::Char('t'));
    handle_normal_key(&mut app, &mut input, KeyCode::Char('s'));
    assert!(app.time_panel.is_none());
    assert_eq!(app.status_msg.as_deref(), Some("UNKNOWN"));

    // Live modes: bare `t` must not arm time operator.
    app.export_source = export::ExportSource::Hdc { device: None };
    app.status_msg = None;
    drain_lines(&mut app, &["04-02 10:00:00.000  1  1 I Tag     : a"]);
    handle_normal_key(&mut app, &mut input, KeyCode::Char('t'));
    assert_ne!(app.pending, Pending::Time);
    assert!(app.time_panel.is_none());

    app.export_source = export::ExportSource::Adb { device: None };
    handle_normal_key(&mut app, &mut input, KeyCode::Char('t'));
    assert_ne!(app.pending, Pending::Time);
    assert!(app.time_panel.is_none());
}

#[test]
fn test_time_panel_ctrl_c_cancels_without_apply() {
    let mut app = App::new(100);
    app.export_source = export::ExportSource::File("demo.log".into());
    drain_lines(&mut app, &["04-02 10:00:00.000  1  1 I Tag     : a"]);
    app.time_bound = Some(crate::filter_model::TimeBound {
        since: Some("04-02 10:00:00".into()),
        until: None,
    });
    assert!(app.open_time_panel());
    handle_time_panel_key(
        &mut app,
        KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
    );
    assert!(app.time_panel.is_none());
    assert_eq!(
        app.time_bound.as_ref().and_then(|t| t.since.as_deref()),
        Some("04-02 10:00:00"),
        "Ctrl+C must not clear the already-applied bound"
    );
}

#[test]
fn test_ma_toggles_and_mm_opens_compare_not_picker() {
    let mut app = App::new(100);
    let mut input = input::InputBox::default();
    drain_lines(
        &mut app,
        &[
            "04-02 10:00:00.000  1  1 I TagA    : first",
            "04-02 10:00:01.000  1  1 I TagB    : second",
        ],
    );
    app.following = false;
    app.cursor = 0;
    handle_normal_key(&mut app, &mut input, KeyCode::Char('m'));
    assert_eq!(app.pending, Pending::Mark);
    handle_normal_key(&mut app, &mut input, KeyCode::Char('a'));
    assert_eq!(app.bookmarks.len(), 1);
    assert_eq!(app.status_msg.as_deref(), Some("BOOKMARKED"));
    handle_normal_key(&mut app, &mut input, KeyCode::Char('m'));
    handle_normal_key(&mut app, &mut input, KeyCode::Char('a'));
    assert!(app.bookmarks.is_empty());
    assert_eq!(app.status_msg.as_deref(), Some("REMOVED"));
    handle_normal_key(&mut app, &mut input, KeyCode::Char('m'));
    handle_normal_key(&mut app, &mut input, KeyCode::Char('a'));
    app.cursor = 1;
    handle_normal_key(&mut app, &mut input, KeyCode::Char('m'));
    handle_normal_key(&mut app, &mut input, KeyCode::Char('m'));
    assert!(app.picker.is_none());
    assert!(app.compare.is_some());
    handle_compare_key(
        &mut app,
        event::KeyEvent::new(KeyCode::Esc, event::KeyModifiers::NONE),
    );
    assert!(app.compare.is_none());
    assert!(!app.following);
    app.cursor = 0;
    handle_normal_key(&mut app, &mut input, KeyCode::Char('m'));
    handle_normal_key(&mut app, &mut input, KeyCode::Char('d'));
    assert!(app.bookmarks.is_empty());
    assert_eq!(app.status_msg.as_deref(), Some("REMOVED"));
}

#[test]
fn compare_panel_enter_jumps_to_row_and_closes() {
    let mut app = App::new(100);
    let mut input = input::InputBox::default();
    drain_lines(
        &mut app,
        &[
            "04-02 10:00:00.000  1  1 I TagA    : first",
            "04-02 10:00:01.000  1  1 I TagB    : second",
        ],
    );
    app.following = false;
    app.cursor = 0;
    app.bookmark_add_current();
    app.cursor = 1;
    handle_normal_key(&mut app, &mut input, KeyCode::Char('m'));
    handle_normal_key(&mut app, &mut input, KeyCode::Char('m'));
    assert!(app.compare.is_some());
    handle_compare_key(
        &mut app,
        event::KeyEvent::new(KeyCode::Enter, event::KeyModifiers::NONE),
    );
    assert!(app.compare.is_none(), "Enter closes the compare panel");
    assert_eq!(app.focus, app::Focus::LogList);
    assert_eq!(app.cursor, 0);
}

#[test]
fn compare_panel_swallows_slash_wrap_and_fast_scroll() {
    let mut app = App::new(100);
    let mut input = input::InputBox::default();
    drain_lines(&mut app, &["04-02 10:00:00.000  1  1 I Tag     : x"]);
    app.bookmark_add_current();
    handle_normal_key(&mut app, &mut input, KeyCode::Char('m'));
    handle_normal_key(&mut app, &mut input, KeyCode::Char('m'));
    assert!(app.compare.is_some());
    let collapsed = app.collapsed_view;
    handle_compare_key(
        &mut app,
        event::KeyEvent::new(KeyCode::Char('/'), event::KeyModifiers::NONE),
    );
    handle_compare_key(
        &mut app,
        event::KeyEvent::new(KeyCode::Char('w'), event::KeyModifiers::NONE),
    );
    handle_compare_key(
        &mut app,
        event::KeyEvent::new(KeyCode::Char('J'), event::KeyModifiers::NONE),
    );
    assert!(app.compare.is_some());
    assert!(app.picker.is_none());
    assert_eq!(app.collapsed_view, collapsed);
}

#[test]
fn compare_panel_dd_deletes_without_confirm() {
    let mut app = App::new(100);
    let mut input = input::InputBox::default();
    drain_lines(&mut app, &["04-02 10:00:00.000  1  1 I Tag     : x"]);
    app.bookmark_add_current();
    assert_eq!(app.bookmarks.len(), 1);
    handle_normal_key(&mut app, &mut input, KeyCode::Char('m'));
    handle_normal_key(&mut app, &mut input, KeyCode::Char('m'));
    handle_compare_key(
        &mut app,
        event::KeyEvent::new(KeyCode::Char('d'), event::KeyModifiers::NONE),
    );
    assert!(app.compare.as_ref().unwrap().pending_d);
    handle_compare_key(
        &mut app,
        event::KeyEvent::new(KeyCode::Char('d'), event::KeyModifiers::NONE),
    );
    assert!(app.bookmarks.is_empty(), "dd deletes without confirm");
    assert!(app.bookmark_row_ids.is_empty(), "row-id cache cleared");
    assert!(app.compare.is_none(), "last dd closes the panel");
}

#[test]
fn compare_panel_yy_yanks_snapshot_raw() {
    let mut app = App::new(100);
    let mut input = input::InputBox::default();
    drain_lines(&mut app, &["04-02 10:00:00.000  1  1 I Tag     : yank-me"]);
    app.bookmark_add_current();
    handle_normal_key(&mut app, &mut input, KeyCode::Char('m'));
    handle_normal_key(&mut app, &mut input, KeyCode::Char('m'));
    handle_compare_key(
        &mut app,
        event::KeyEvent::new(KeyCode::Char('y'), event::KeyModifiers::NONE),
    );
    handle_compare_key(
        &mut app,
        event::KeyEvent::new(KeyCode::Char('y'), event::KeyModifiers::NONE),
    );
    let yanked = app.last_yanked.as_deref().unwrap_or("");
    assert!(yanked.contains("yank-me"), "{yanked}");
}

#[test]
fn compare_panel_ctrl_c_closes_without_resume() {
    let mut app = App::new(100);
    let mut input = input::InputBox::default();
    drain_lines(&mut app, &["04-02 10:00:00.000  1  1 I Tag     : x"]);
    app.bookmark_add_current();
    app.following = false;
    handle_normal_key(&mut app, &mut input, KeyCode::Char('m'));
    handle_normal_key(&mut app, &mut input, KeyCode::Char('m'));
    handle_compare_key(
        &mut app,
        event::KeyEvent::new(KeyCode::Char('c'), event::KeyModifiers::CONTROL),
    );
    assert!(app.compare.is_none());
    assert!(!app.following);
}

#[test]
fn visual_ma_does_not_bulk_pin() {
    let mut app = App::new(100);
    drain_lines(
        &mut app,
        &[
            "04-02 10:00:00.000  1  1 I TagA    : first",
            "04-02 10:00:01.000  1  1 I TagB    : second",
            "04-02 10:00:02.000  1  1 I TagC    : third",
        ],
    );
    app.following = false;
    app.cursor = 0;
    app.enter_visual_line();
    app.cursor = 2;
    app.bookmark_add_current();
    assert_eq!(app.bookmarks.len(), 1);
    assert_eq!(app.bookmarks.items[0].row_id(), app.view_source()[2].row_id);
}

#[test]
fn bookmark_row_ids_cache_synced_on_add_and_remove() {
    // F1: the O(1) bookmark-row cache stays in sync with the bookmark list.
    let mut app = App::new(100);
    drain_lines(
        &mut app,
        &[
            "04-02 10:00:00.000  1  1 I TagA    : first",
            "04-02 10:00:01.000  1  1 I TagB    : second",
        ],
    );
    let rid0 = app.view_source()[app.source_idx_for_visible(0).unwrap()].row_id;
    app.cursor = 0;
    app.bookmark_add_current();
    assert!(app.is_bookmark_row(rid0));
    assert!(app.bookmark_row_ids.contains(&rid0));
    app.bookmark_remove_current();
    assert!(!app.is_bookmark_row(rid0));
    assert!(!app.bookmark_row_ids.contains(&rid0));
}

#[test]
fn test_ma_does_not_enter_insert() {
    let mut app = App::new(100);
    let mut input = input::InputBox::default();
    drain_lines(&mut app, &["04-02 10:00:00.000  1  1 I Tag     : x"]);
    handle_normal_key(&mut app, &mut input, KeyCode::Char('m'));
    handle_normal_key(&mut app, &mut input, KeyCode::Char('a'));
    assert_eq!(app.mode, app::Mode::Normal);
    assert_eq!(app.focus, app::Focus::LogList);
    assert_eq!(app.bookmarks.len(), 1);
}

#[test]
fn ctrl_s_opens_save_dialog_and_ctrl_o_applies_preset() {
    use crate::input::{build_group_from_chips, Chip, ChipField};
    use tempfile::TempDir;

    let dir = TempDir::new().unwrap();
    let mut app = App::new(100);
    app.config_dir = dir.path().to_path_buf();
    let mut input = input::InputBox::default();
    drain_lines(
        &mut app,
        &[
            "04-02 10:00:00.000  1  1 E MyApp   : error one",
            "04-02 10:00:01.000  1  1 I Other   : ok",
        ],
    );
    app.groups.groups.push(
        build_group_from_chips(
            vec![Chip {
                field: ChipField::Tag,
                value: "MyApp".into(),
            }],
            true,
        )
        .unwrap()
        .unwrap(),
    );
    app.rebuild_visible();
    app.following = true;

    handle_normal_event(
        &mut app,
        &mut input,
        KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL),
    );
    assert!(app.preset_name.is_some());
    {
        let d = app.preset_name.as_mut().unwrap();
        for c in "crash-login".chars() {
            d.field.insert(c);
        }
    }
    handle_preset_name_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(app.preset_name.is_none());
    assert!(crate::preset::exists(dir.path(), "crash-login"));
    assert_eq!(app.status_msg.as_deref(), Some("PRESET SAVED"));

    app.groups.groups.clear();
    app.rebuild_visible();
    assert_eq!(app.groups.groups.len(), 0);

    handle_normal_event(
        &mut app,
        &mut input,
        KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL),
    );
    assert!(matches!(
        app.picker.as_ref().map(|p| &p.kind),
        Some(crate::picker::PickerKind::Preset)
    ));
    let data = picker_render_data(&app, 10, 80).unwrap();
    assert!(data.show_preview);
    assert!(data.preset_preview.is_some());
    handle_picker_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(app.picker.is_none());
    assert_eq!(app.groups.groups.len(), 1);
    assert_eq!(app.groups.groups[0].chips[0].value, "MyApp");
    assert!(!app.following);
    assert_eq!(app.status_msg.as_deref(), Some("PRESET APPLIED"));
}

#[test]
fn ctrl_o_empty_library_flashes_without_opening() {
    use tempfile::TempDir;

    let dir = TempDir::new().unwrap();
    let mut app = App::new(100);
    app.config_dir = dir.path().to_path_buf();
    let mut input = input::InputBox::default();
    handle_normal_event(
        &mut app,
        &mut input,
        KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL),
    );
    assert!(app.picker.is_none());
    assert_eq!(app.status_msg.as_deref(), Some("NO PRESETS"));
}

#[test]
fn apply_preset_keeps_time_lock_and_bookmarks() {
    use crate::filter_model::{GroupList, TimeBound};
    use crate::highlight_model::HighlightGroupList;
    use crate::input::{build_group_from_chips, Chip, ChipField};

    let mut app = App::new(100);
    drain_lines(&mut app, &["04-02 10:00:00.000  9  8 E MyApp   : boom"]);
    app.lock_pid = Some("9".into());
    app.time_bound = Some(TimeBound {
        since: Some("04-02 10:00:00.000".into()),
        until: None,
    });
    app.bookmark_add_current();
    let bm_len = app.bookmarks.len();

    let preset = crate::preset::capture(
        &GroupList {
            groups: vec![build_group_from_chips(
                vec![Chip {
                    field: ChipField::Tag,
                    value: "MyApp".into(),
                }],
                true,
            )
            .unwrap()
            .unwrap()],
            excludes: vec![],
        },
        &HighlightGroupList::default(),
        "keep-extra",
    )
    .unwrap()
    .unwrap();
    app.apply_preset(&preset).unwrap();
    assert_eq!(app.lock_pid.as_deref(), Some("9"));
    assert!(app.time_bound.is_some());
    assert_eq!(app.bookmarks.len(), bm_len);
    assert_eq!(app.groups.groups.len(), 1);
}
