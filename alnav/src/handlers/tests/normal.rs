//! Normal-mode LogList / strip / yank / chip / lock / detail key tests.

use super::*;
use crate::app::App;
use crate::app::Pending;
use crate::filter_model::Group;
use crate::handlers::normal::handle_normal_key;
use crate::handlers::picker::keys::handle_picker_key;
use crate::handlers::{handle_ctrl_c, handle_mouse_event};
use crate::{app, export, highlight_model, model, ui};
use crossterm::event::KeyModifiers;
use crossterm::event::{self, KeyCode};

#[test]
fn test_aio_no_longer_enter_insert() {
    let mut app = App::new(100);

    for c in ['a', 'i', 'o'] {
        handle_normal_key(&mut app, KeyCode::Char(c));

        assert_eq!(app.focus, app::Focus::LogList, "{c} must not open Input");
    }
}

#[test]
fn test_number_keys_switch_focus_and_5_opens_unified_picker() {
    let mut app = App::new(100);

    handle_normal_key(&mut app, KeyCode::Char('5'));
    assert!(matches!(
        app.picker.as_ref().map(|picker| &picker.kind),
        Some(crate::picker::PickerKind::Unified)
    ));
    app.close_picker();

    handle_normal_key(&mut app, KeyCode::Char('1'));
    assert_eq!(app.focus, app::Focus::ChipStrip);

    handle_normal_key(&mut app, KeyCode::Char('2'));
    assert_eq!(app.focus, app::Focus::ExcludeStrip);

    handle_normal_key(&mut app, KeyCode::Char('3'));
    assert_eq!(app.focus, app::Focus::HighlightStrip);

    handle_normal_key(&mut app, KeyCode::Char('4'));
    assert_eq!(app.focus, app::Focus::LogList);
}

#[test]
fn test_esc_from_other_focus_preserves_loglist_position() {
    let mut app = App::new(100);

    let (tx, rx) = std::sync::mpsc::channel();
    for i in 0..20 {
        tx.send(
            model::EntryRow::from_line(&format!("04-02 10:00:00.000  1  1 I Tag     : line{i}"))
                .unwrap(),
        )
        .unwrap();
    }
    drop(tx);
    app.drain(&rx);
    app.cursor = 5;
    app.list_offset = 2;
    app.following = false;
    app.focus = app::Focus::ChipStrip;
    handle_normal_key(&mut app, KeyCode::Esc);
    assert_eq!(app.focus, app::Focus::LogList);
    assert!(
        !app.following,
        "Esc from ChipStrip must not resume following"
    );
    assert_eq!(app.cursor, 5);
    assert_eq!(app.list_offset, 2);
}

#[test]
fn test_esc_on_loglist_resumes_follow() {
    let mut app = App::new(100);

    let (tx, rx) = std::sync::mpsc::channel();
    tx.send(model::EntryRow::from_line("04-02 10:00:00.000  1  1 I T   : a").unwrap())
        .unwrap();
    tx.send(model::EntryRow::from_line("04-02 10:00:01.000  1  1 I T   : b").unwrap())
        .unwrap();
    drop(tx);
    app.drain(&rx);
    app.move_cursor_manual(-1);
    assert!(!app.following);
    assert_eq!(app.cursor, 0);
    handle_normal_key(&mut app, KeyCode::Esc);
    assert!(app.following);
    assert_eq!(app.cursor, 1);
}

#[test]
fn test_shift_j_and_shift_k_move_cursor_by_seven() {
    let mut app = App::new(100);

    let (tx, rx) = std::sync::mpsc::channel();
    for i in 0..20 {
        tx.send(
            crate::model::EntryRow::from_line(&format!(
                "04-02 10:00:00.000  1  1 I Tag     : line{i}"
            ))
            .unwrap(),
        )
        .unwrap();
    }
    drop(tx);
    app.drain(&rx);
    app.cursor = 10;
    app.following = false;

    handle_normal_key(&mut app, KeyCode::Char('K'));
    assert_eq!(app.cursor, 3);

    handle_normal_key(&mut app, KeyCode::Char('J'));
    assert_eq!(app.cursor, 10);
}

#[test]
fn test_ctrl_c_in_normal_mode_quits() {
    let mut app = App::new(100);

    handle_ctrl_c(&mut app);
    assert!(app.should_quit);
}

#[test]
fn test_dd_chord_requires_two_consecutive_d_presses() {
    let mut app = App::new(100);

    app.groups.groups.push(Group {
        label: "g0".into(),
        chips: Vec::new(),
        enabled: true,
        same_field_op: crate::fuzzy::SameFieldOp::And,
    });
    app.focus = app::Focus::ChipStrip;
    handle_normal_key(&mut app, KeyCode::Char('d'));
    assert_eq!(app.pending, Pending::StripD);
    handle_normal_key(&mut app, KeyCode::Char('j')); // unrelated key in between
    assert!(
        app.pending != Pending::StripD,
        "pending_d should clear on a non-d key"
    );
    assert_eq!(app.groups.groups.len(), 1, "group should NOT be deleted");
}

#[test]
fn test_di_toggles_filter_group_disabled() {
    let mut app = App::new(100);

    app.groups.groups.push(Group {
        label: "g0".into(),
        chips: Vec::new(),
        enabled: true,
        same_field_op: crate::fuzzy::SameFieldOp::And,
    });
    app.focus = app::Focus::ChipStrip;
    handle_normal_key(&mut app, KeyCode::Char('d'));
    handle_normal_key(&mut app, KeyCode::Char('i'));
    assert!(!app.groups.groups[0].enabled);

    assert_eq!(app.focus, app::Focus::ChipStrip);
    handle_normal_key(&mut app, KeyCode::Char('d'));
    handle_normal_key(&mut app, KeyCode::Char('i'));
    assert!(app.groups.groups[0].enabled);
}

#[test]
fn test_g_jump_bottom_resumes_follow() {
    let mut app = App::new(100);

    let (tx, rx) = std::sync::mpsc::channel();
    tx.send(model::EntryRow::from_line("04-02 10:00:00.000  1  1 I T   : a").unwrap())
        .unwrap();
    tx.send(model::EntryRow::from_line("04-02 10:00:01.000  1  1 I T   : b").unwrap())
        .unwrap();
    drop(tx);
    app.drain(&rx);
    app.move_cursor_manual(-1);
    assert!(!app.following);
    handle_normal_key(&mut app, KeyCode::Char('G'));
    assert_eq!(app.cursor, 1);
    assert!(app.following, "G jumps to bottom and resumes following");
}

#[test]
fn test_j_to_bottom_resumes_follow() {
    let mut app = App::new(100);

    let (tx, rx) = std::sync::mpsc::channel();
    tx.send(model::EntryRow::from_line("04-02 10:00:00.000  1  1 I T   : a").unwrap())
        .unwrap();
    tx.send(model::EntryRow::from_line("04-02 10:00:01.000  1  1 I T   : b").unwrap())
        .unwrap();
    drop(tx);
    app.drain(&rx);
    app.move_cursor_manual(-1);
    assert!(!app.following);
    handle_normal_key(&mut app, KeyCode::Char('j'));
    assert_eq!(app.cursor, 1);
    assert!(app.following, "j to bottom resumes following");
}

#[test]
fn test_shift_j_to_bottom_resumes_follow() {
    let mut app = App::new(100);

    let (tx, rx) = std::sync::mpsc::channel();
    for i in 0..10 {
        tx.send(
            model::EntryRow::from_line(&format!("04-02 10:00:00.000  1  1 I Tag     : line{i}"))
                .unwrap(),
        )
        .unwrap();
    }
    drop(tx);
    app.drain(&rx);
    app.cursor = 5;
    app.following = false;
    handle_normal_key(&mut app, KeyCode::Char('J'));
    assert_eq!(app.cursor, 9);
    assert!(app.following, "J landing on bottom resumes following");
}

#[test]
fn test_mouse_scroll_down_and_up_move_cursor_by_three() {
    let mut app = App::new(100);
    let (tx, rx) = std::sync::mpsc::channel();
    for i in 0..20 {
        tx.send(
            crate::model::EntryRow::from_line(&format!(
                "04-02 10:00:00.000  1  1 I Tag     : line{i}"
            ))
            .unwrap(),
        )
        .unwrap();
    }
    drop(tx);
    app.drain(&rx);
    app.cursor = 10;
    app.following = false;

    handle_mouse_event(
        &mut app,
        event::MouseEvent {
            kind: event::MouseEventKind::ScrollUp,
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        },
    );
    assert_eq!(app.cursor, 7);

    handle_mouse_event(
        &mut app,
        event::MouseEvent {
            kind: event::MouseEventKind::ScrollDown,
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        },
    );
    assert_eq!(app.cursor, 10);
}

#[test]
fn test_mouse_click_is_ignored() {
    let mut app = App::new(100);
    let (tx, rx) = std::sync::mpsc::channel();
    tx.send(crate::model::EntryRow::from_line("04-02 10:00:00.000  1  1 I Tag     : a").unwrap())
        .unwrap();
    tx.send(crate::model::EntryRow::from_line("04-02 10:00:01.000  1  1 I Tag     : b").unwrap())
        .unwrap();
    drop(tx);
    app.drain(&rx);
    let cursor_before = app.cursor;

    handle_mouse_event(
        &mut app,
        event::MouseEvent {
            kind: event::MouseEventKind::Down(event::MouseButton::Left),
            column: 5,
            row: 5,
            modifiers: KeyModifiers::NONE,
        },
    );
    assert_eq!(app.cursor, cursor_before, "clicks are ignored in this pass");
}

#[test]
fn test_yy_yanks_raw_and_clears_pending() {
    let mut app = App::new(100);

    let line = "04-02 10:00:00.000  1  1 I Tag     : hello";
    drain_lines(&mut app, &[line]);
    handle_normal_key(&mut app, KeyCode::Char('y'));
    assert_eq!(app.pending, Pending::Yank);
    handle_normal_key(&mut app, KeyCode::Char('y'));
    assert_ne!(app.pending, Pending::Yank);
    assert_eq!(app.last_yanked.as_deref(), Some(line));
}

#[test]
fn test_yt_and_ym_yank_fields() {
    use crate::picker::{MsgChipPurpose, PickerKind, PickerMode};

    let mut app = App::new(100);

    drain_lines(
        &mut app,
        &["04-02 10:00:00.000  1  1 I MyTag   : hello boom world"],
    );
    handle_normal_key(&mut app, KeyCode::Char('y'));
    handle_normal_key(&mut app, KeyCode::Char('t'));
    assert_eq!(app.last_yanked.as_deref(), Some("MyTag"));
    handle_normal_key(&mut app, KeyCode::Char('y'));
    handle_normal_key(&mut app, KeyCode::Char('m'));
    assert!(matches!(
        app.picker.as_ref().map(|p| (&p.kind, &p.mode)),
        Some((
            PickerKind::MsgChip {
                purpose: MsgChipPurpose::Yank
            },
            PickerMode::New
        ))
    ));
    for c in "boom".chars() {
        handle_picker_key(
            &mut app,
            event::KeyEvent::new(KeyCode::Char(c), event::KeyModifiers::NONE),
        );
    }
    handle_picker_key(
        &mut app,
        event::KeyEvent::new(KeyCode::Enter, event::KeyModifiers::NONE),
    );
    assert!(app.picker.is_none());
    assert_eq!(app.last_yanked.as_deref(), Some("boom"));
}

#[test]
fn test_capital_y_yanks_msg() {
    let mut app = App::new(100);

    drain_lines(&mut app, &["04-02 10:00:00.000  1  1 I Tag     : onlymsg"]);
    handle_normal_key(&mut app, KeyCode::Char('Y'));
    assert_eq!(app.last_yanked.as_deref(), Some("onlymsg"));
    assert_ne!(app.pending, Pending::Yank);
}

#[test]
fn test_yank_pending_esc_cancels() {
    let mut app = App::new(100);

    drain_lines(&mut app, &["04-02 10:00:00.000  1  1 I Tag     : x"]);
    handle_normal_key(&mut app, KeyCode::Char('y'));
    handle_normal_key(&mut app, KeyCode::Esc);
    assert_ne!(app.pending, Pending::Yank);
    assert!(app.last_yanked.is_none());
}

#[test]
fn test_visual_line_shift_v_jk_y_yanks_range() {
    let mut app = App::new(100);

    let a = "04-02 10:00:00.000  1  1 I Tag     : a";
    let b = "04-02 10:00:01.000  1  1 I Tag     : b";
    let c = "04-02 10:00:02.000  1  1 I Tag     : c";
    drain_lines(&mut app, &[a, b, c]);
    app.following = false;
    app.cursor = 0;
    handle_normal_key(&mut app, KeyCode::Char('V'));
    assert_eq!(app.visual_anchor, Some(0));
    handle_normal_key(&mut app, KeyCode::Char('j'));
    handle_normal_key(&mut app, KeyCode::Char('j'));
    assert_eq!(app.cursor, 2);
    handle_normal_key(&mut app, KeyCode::Char('y'));
    assert!(app.visual_anchor.is_none());
    let expected = format!("{a}\n{b}\n{c}");
    assert_eq!(app.last_yanked.as_deref(), Some(expected.as_str()));
}

#[test]
fn test_n_and_shift_n_jump_highlight_matches() {
    let mut app = App::new(100);

    drain_lines(
        &mut app,
        &[
            "04-02 10:00:00.000  1  1 I Tag     : aaa",
            "04-02 10:00:01.000  1  1 I Tag     : hit one",
            "04-02 10:00:02.000  1  1 I Tag     : bbb",
            "04-02 10:00:03.000  1  1 I Tag     : hit two",
        ],
    );
    app.following = false;
    app.cursor = 0;
    app.push_or_find_highlight_group(highlight_model::HighlightGroup::from_pattern("hit").unwrap());
    handle_normal_key(&mut app, KeyCode::Char('n'));
    assert_eq!(app.cursor, 1);
    handle_normal_key(&mut app, KeyCode::Char('n'));
    assert_eq!(app.cursor, 3);
    handle_normal_key(&mut app, KeyCode::Char('n'));
    assert_eq!(app.cursor, 3);
    assert_eq!(app.status_msg.as_deref(), Some("NO MORE"));
    app.status_msg = None;
    handle_normal_key(&mut app, KeyCode::Char('N'));
    assert_eq!(app.cursor, 1);
    handle_normal_key(&mut app, KeyCode::Char('N'));
    assert_eq!(app.cursor, 1);
    assert_eq!(app.status_msg.as_deref(), Some("NO MORE"));
}

#[test]
fn test_e_and_shift_e_jump_severe_rows() {
    let mut app = App::new(100);

    drain_lines(
        &mut app,
        &[
            "04-02 10:00:00.000  1  1 I Tag     : aaa",
            "04-02 10:00:01.000  1  1 E Tag     : err one",
            "04-02 10:00:02.000  1  1 I Tag     : bbb",
            "04-02 10:00:03.000  1  1 F Tag     : fatal",
        ],
    );
    app.following = false;
    app.cursor = 0;
    handle_normal_key(&mut app, KeyCode::Char('e'));
    assert_eq!(app.cursor, 1);
    handle_normal_key(&mut app, KeyCode::Char('e'));
    assert_eq!(app.cursor, 3);
    handle_normal_key(&mut app, KeyCode::Char('e'));
    assert_eq!(app.cursor, 3);
    assert_eq!(app.status_msg.as_deref(), Some("NO MORE"));
    app.status_msg = None;
    handle_normal_key(&mut app, KeyCode::Char('E'));
    assert_eq!(app.cursor, 1);
    assert_ne!(app.status_msg.as_deref(), Some("NO ERROR"));
}

#[test]
fn test_e_sets_no_error_when_no_severe() {
    let mut app = App::new(100);

    drain_lines(&mut app, &["04-02 10:00:00.000  1  1 I Tag     : aaa"]);
    app.following = false;
    app.cursor = 0;
    handle_normal_key(&mut app, KeyCode::Char('e'));
    assert_eq!(app.cursor, 0);
    assert_eq!(app.status_msg.as_deref(), Some("NO ERROR"));
}

#[test]
fn test_e_does_not_interfere_with_search_n() {
    let mut app = App::new(100);

    drain_lines(
        &mut app,
        &[
            "04-02 10:00:00.000  1  1 I Tag     : hit info",
            "04-02 10:00:01.000  1  1 E Tag     : other err",
            "04-02 10:00:02.000  1  1 I Tag     : hit two",
        ],
    );
    app.following = false;
    app.cursor = 0;
    app.push_or_find_highlight_group(highlight_model::HighlightGroup::from_pattern("hit").unwrap());
    handle_normal_key(&mut app, KeyCode::Char('n'));
    assert_eq!(app.cursor, 2, "n follows search, not severe");
    // Severe is behind the cursor; forward `e` no longer wraps.
    handle_normal_key(&mut app, KeyCode::Char('E'));
    assert_eq!(app.cursor, 1, "E follows severe, not search");
}

#[test]
fn test_ct_pushes_tag_chip_and_narrows_visible() {
    let mut app = App::new(100);

    drain_lines(
        &mut app,
        &[
            "04-02 10:00:00.000  1  1 I Keep    : a",
            "04-02 10:00:01.000  1  1 I Drop    : b",
            "04-02 10:00:02.000  1  1 I Keep    : c",
        ],
    );
    app.following = true;
    app.cursor = 0;
    handle_normal_key(&mut app, KeyCode::Char('c'));
    assert_eq!(app.pending, Pending::Chip);
    handle_normal_key(&mut app, KeyCode::Char('t'));
    assert_ne!(app.pending, Pending::Chip);
    assert_eq!(app.groups.groups.len(), 1);
    assert_eq!(app.groups.groups[0].chips[0].value, "Keep");
    assert_eq!(app.index.visible.len(), 2);
    assert!(!app.following, "chip-from-cursor leaves following off");
    assert_eq!(app.status_msg.as_deref(), Some("FILTER"));
}

#[test]
fn test_cl_uses_level_gte_semantics() {
    let mut app = App::new(100);

    drain_lines(
        &mut app,
        &[
            "04-02 10:00:00.000  1  1 I Tag     : info",
            "04-02 10:00:01.000  1  1 W Tag     : warn",
            "04-02 10:00:02.000  1  1 E Tag     : err",
            "04-02 10:00:03.000  1  1 F Tag     : fatal",
        ],
    );
    app.following = false;
    app.cursor = 1; // W
    handle_normal_key(&mut app, KeyCode::Char('c'));
    handle_normal_key(&mut app, KeyCode::Char('l'));
    assert_eq!(app.index.visible.len(), 3, "level>=W keeps W/E/F");
    let levels: Vec<_> = (0..app.index.visible.len())
        .filter_map(|i| app.row_at(i).map(|r| r.level.as_char()))
        .collect();
    assert_eq!(levels, vec!['W', 'E', 'F']);
}

#[test]
fn test_ct_duplicate_does_not_push() {
    let mut app = App::new(100);

    drain_lines(&mut app, &["04-02 10:00:00.000  1  1 I MyTag   : x"]);
    handle_normal_key(&mut app, KeyCode::Char('c'));
    handle_normal_key(&mut app, KeyCode::Char('t'));
    assert_eq!(app.groups.groups.len(), 1);
    handle_normal_key(&mut app, KeyCode::Char('c'));
    handle_normal_key(&mut app, KeyCode::Char('t'));
    assert_eq!(app.groups.groups.len(), 1);
    assert_eq!(app.status_msg.as_deref(), Some("EXISTS"));
}

#[test]
fn test_ym_draft_fallback_yanks_draft() {
    let mut app = App::new(100);

    drain_lines(
        &mut app,
        &["04-02 10:00:00.000  1  1 I Tag     : hello world"],
    );
    handle_normal_key(&mut app, KeyCode::Char('y'));
    handle_normal_key(&mut app, KeyCode::Char('m'));
    for c in "customxyz".chars() {
        handle_picker_key(
            &mut app,
            event::KeyEvent::new(KeyCode::Char(c), event::KeyModifiers::NONE),
        );
    }
    handle_picker_key(
        &mut app,
        event::KeyEvent::new(KeyCode::Enter, event::KeyModifiers::NONE),
    );
    assert!(app.picker.is_none());
    assert_eq!(app.last_yanked.as_deref(), Some("customxyz"));
}

#[test]
fn test_chip_pending_esc_does_not_resume_following() {
    let mut app = App::new(100);

    drain_lines(&mut app, &["04-02 10:00:00.000  1  1 I Tag     : x"]);
    app.following = false;
    handle_normal_key(&mut app, KeyCode::Char('c'));
    assert_eq!(app.pending, Pending::Chip);
    handle_normal_key(&mut app, KeyCode::Esc);
    assert_ne!(app.pending, Pending::Chip);
    assert!(
        !app.following,
        "Esc while pending_c must not resume following"
    );
    assert!(app.groups.groups.is_empty());
}

#[test]
fn test_c_unsupported_and_unknown_field() {
    let mut app = App::new(100);

    drain_lines(&mut app, &["04-02 10:00:00.000  1  1 I Tag     : x"]);
    handle_normal_key(&mut app, KeyCode::Char('c'));
    handle_normal_key(&mut app, KeyCode::Char('s'));
    assert_eq!(app.status_msg.as_deref(), Some("NO RAW/TIMESTAMP"));
    handle_normal_key(&mut app, KeyCode::Char('c'));
    handle_normal_key(&mut app, KeyCode::Char('x'));
    assert_eq!(app.status_msg.as_deref(), Some("UNKNOWN FIELD"));
    assert!(app.groups.groups.is_empty());
}

#[test]
fn test_fh_fe_view_focus_keys() {
    let mut app = App::new(100);

    drain_lines(
        &mut app,
        &[
            "04-02 10:00:00.000  1  1 I Tag     : hit one",
            "04-02 10:00:01.000  1  1 E Tag     : hit err",
            "04-02 10:00:02.000  1  1 E Tag     : plain err",
            "04-02 10:00:03.000  1  1 I Tag     : other",
        ],
    );
    app.push_or_find_highlight_group(highlight_model::HighlightGroup::from_pattern("hit").unwrap());
    app.following = true;
    handle_normal_key(&mut app, KeyCode::Char('f'));
    handle_normal_key(&mut app, KeyCode::Char('h'));
    assert!(app.view_focus.highlight);
    assert!(!app.view_focus.severe);
    assert_eq!(app.index.visible.len(), 2);
    assert!(!app.following);

    // fe stacks: intersection of highlight ∩ severe.
    handle_normal_key(&mut app, KeyCode::Char('f'));
    handle_normal_key(&mut app, KeyCode::Char('e'));
    assert!(app.view_focus.highlight);
    assert!(app.view_focus.severe);
    assert_eq!(app.index.visible.len(), 1);
    assert_eq!(app.current_row().unwrap().msg, "hit err");

    app.following = false;
    handle_normal_key(&mut app, KeyCode::Esc);
    assert!(app.following);
    assert!(app.view_focus.highlight && app.view_focus.severe);

    handle_normal_key(&mut app, KeyCode::Char('f'));
    handle_normal_key(&mut app, KeyCode::Char('e'));
    assert!(app.view_focus.highlight);
    assert!(!app.view_focus.severe);
    assert_eq!(app.index.visible.len(), 2);

    handle_normal_key(&mut app, KeyCode::Char('f'));
    handle_normal_key(&mut app, KeyCode::Char('h'));
    assert!(!app.view_focus.is_active());
    assert_eq!(app.index.visible.len(), 4);
}

#[test]
fn test_fp_locks_pid_and_ft_clears_pid_lock() {
    let mut app = App::new(100);

    drain_lines(
        &mut app,
        &[
            "04-02 10:00:00.000  111  1 I Tag     : a",
            "04-02 10:00:01.000  222  2 I Tag     : b",
            "04-02 10:00:02.000  111  3 I Tag     : c",
        ],
    );
    app.following = true;
    app.cursor = 0;
    handle_normal_key(&mut app, KeyCode::Char('f'));
    assert_eq!(app.pending, Pending::Lock);
    handle_normal_key(&mut app, KeyCode::Char('p'));
    assert_eq!(app.lock_pid.as_deref(), Some("111"));
    assert!(app.lock_tid.is_none());
    assert_eq!(app.index.visible.len(), 2);
    assert!(app.following, "lock coexists with following");
    assert_eq!(app.lock_badge_label().as_deref(), Some("pid=111"));

    app.cursor = 0; // still on a pid=111 row after rebuild+follow
                    // switch to tid lock on current row (tid=1 on first visible)
    let tid = app.current_row().unwrap().tid.clone();
    handle_normal_key(&mut app, KeyCode::Char('f'));
    handle_normal_key(&mut app, KeyCode::Char('t'));
    assert!(app.lock_pid.is_none(), "tid lock clears pid lock");
    assert_eq!(app.lock_tid.as_deref(), Some(tid.as_str()));
}

#[test]
fn test_fp_toggle_same_value_and_fu_clear() {
    let mut app = App::new(100);

    drain_lines(
        &mut app,
        &[
            "04-02 10:00:00.000  111  1 I Tag     : a",
            "04-02 10:00:01.000  222  2 I Tag     : b",
        ],
    );
    app.following = false;
    app.cursor = 0;
    handle_normal_key(&mut app, KeyCode::Char('f'));
    handle_normal_key(&mut app, KeyCode::Char('p'));
    assert_eq!(app.index.visible.len(), 1);
    handle_normal_key(&mut app, KeyCode::Char('f'));
    handle_normal_key(&mut app, KeyCode::Char('p'));
    assert!(app.lock_pid.is_none());
    assert_eq!(app.index.visible.len(), 2);
    assert_eq!(app.status_msg.as_deref(), Some("UNLOCK"));

    app.cursor = 0;
    handle_normal_key(&mut app, KeyCode::Char('f'));
    handle_normal_key(&mut app, KeyCode::Char('p'));
    handle_normal_key(&mut app, KeyCode::Char('f'));
    handle_normal_key(&mut app, KeyCode::Char('u'));
    assert!(app.lock_pid.is_none());
    assert_eq!(app.index.visible.len(), 2);
}

#[test]
fn test_esc_resume_following_keeps_lock() {
    let mut app = App::new(100);

    drain_lines(
        &mut app,
        &[
            "04-02 10:00:00.000  111  1 I Tag     : a",
            "04-02 10:00:01.000  222  2 I Tag     : b",
        ],
    );
    app.following = false;
    app.cursor = 0;
    handle_normal_key(&mut app, KeyCode::Char('f'));
    handle_normal_key(&mut app, KeyCode::Char('p'));
    assert_eq!(app.lock_pid.as_deref(), Some("111"));
    handle_normal_key(&mut app, KeyCode::Esc);
    assert!(app.following);
    assert_eq!(app.lock_pid.as_deref(), Some("111"));
    assert_eq!(app.index.visible.len(), 1);
}

#[test]
fn test_lock_pending_esc_does_not_resume_or_clear_lock() {
    let mut app = App::new(100);

    drain_lines(&mut app, &["04-02 10:00:00.000  111  1 I Tag     : a"]);
    app.following = false;
    app.lock_pid = Some("111".into());
    handle_normal_key(&mut app, KeyCode::Char('f'));
    handle_normal_key(&mut app, KeyCode::Esc);
    assert_ne!(app.pending, Pending::Lock);
    assert!(!app.following);
    assert_eq!(app.lock_pid.as_deref(), Some("111"));
}

#[test]
fn test_ct_exclude_hides_tag_and_dd_clears() {
    let mut app = App::new(100);

    drain_lines(
        &mut app,
        &[
            "04-02 10:00:00.000  1  1 I Keep    : a",
            "04-02 10:00:01.000  1  1 I Spam    : b",
            "04-02 10:00:02.000  1  1 I Keep    : c",
        ],
    );
    app.following = false;
    app.cursor = 1; // Spam
    handle_normal_key(&mut app, KeyCode::Char('C'));
    assert_eq!(app.pending, Pending::Exclude);
    handle_normal_key(&mut app, KeyCode::Char('t'));
    assert_eq!(app.groups.excludes.len(), 1);
    assert!(app.groups.excludes[0]
        .chip
        .value
        .eq_ignore_ascii_case("Spam"));
    assert_eq!(app.index.visible.len(), 2);
    app.focus = app::Focus::ExcludeStrip;
    app.exclude_cursor = 0;
    handle_normal_key(&mut app, KeyCode::Char('d'));
    handle_normal_key(&mut app, KeyCode::Char('d'));
    assert!(app.groups.excludes.is_empty());
    assert_eq!(app.focus, app::Focus::LogList);
    assert_eq!(app.index.visible.len(), 3);
}

#[test]
fn test_exclude_di_restores_visibility() {
    let mut app = App::new(100);

    drain_lines(
        &mut app,
        &[
            "04-02 10:00:00.000  1  1 I Spam    : a",
            "04-02 10:00:01.000  1  1 I Keep    : b",
        ],
    );
    app.following = false;
    app.cursor = 0;
    handle_normal_key(&mut app, KeyCode::Char('C'));
    handle_normal_key(&mut app, KeyCode::Char('t'));
    assert_eq!(app.index.visible.len(), 1);
    app.focus = app::Focus::ExcludeStrip;
    handle_normal_key(&mut app, KeyCode::Char('d'));
    handle_normal_key(&mut app, KeyCode::Char('i'));
    assert!(!app.groups.excludes[0].enabled);
    assert_eq!(app.index.visible.len(), 2);
}

#[test]
fn test_exclude_strip_height_zero_when_empty() {
    let app = App::new(100);
    assert_eq!(ui::exclude_strip_height(&app, 80), 0);
}

#[test]
fn test_p_toggles_detail_fields() {
    use crate::app::DetailView;
    let mut app = App::new(100);

    drain_lines(
        &mut app,
        &[
            "04-02 10:00:00.000  1  1 I TagA    : one",
            "04-02 10:00:01.000  2  2 E TagB    : two",
        ],
    );
    app.following = false;
    app.cursor = 0;
    handle_normal_key(&mut app, KeyCode::Char('p'));
    assert_eq!(app.detail, DetailView::Fields);
    let lines = ui::detail_field_lines(app.current_row().as_deref(), 40);
    let joined: String = lines
        .iter()
        .map(|l| {
            l.spans
                .iter()
                .map(|s| s.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(joined.contains("TagA"), "detail shows selected tag");
    handle_normal_key(&mut app, KeyCode::Char('j'));
    let lines2 = ui::detail_field_lines(app.current_row().as_deref(), 40);
    let joined2: String = lines2
        .iter()
        .map(|l| {
            l.spans
                .iter()
                .map(|s| s.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(joined2.contains("TagB"), "detail follows cursor");
    handle_normal_key(&mut app, KeyCode::Char('p'));
    assert_eq!(app.detail, DetailView::Closed);
}

#[test]
fn test_w_toggles_collapsed_view() {
    let mut app = App::new(100);

    drain_lines(&mut app, &["04-02 10:00:00.000  1  1 I Tag     : hello"]);
    assert!(
        !app.collapsed_view,
        "collapsed_view must default to false (multi-line)"
    );
    let cursor_before = app.cursor;
    let following_before = app.following;
    let list_offset_before = app.list_offset;

    handle_normal_key(&mut app, KeyCode::Char('w'));
    assert!(app.collapsed_view, "w must toggle into collapsed view");
    assert_eq!(app.cursor, cursor_before, "w must not move cursor");
    assert_eq!(
        app.following, following_before,
        "w must not change following"
    );
    assert_eq!(
        app.list_offset, list_offset_before,
        "w must not change list_offset"
    );

    handle_normal_key(&mut app, KeyCode::Char('w'));
    assert!(!app.collapsed_view, "w must toggle back to multi-line");
}

#[test]
fn test_detail_esc_closes_without_resume_following() {
    use crate::app::DetailView;
    let mut app = App::new(100);

    drain_lines(&mut app, &["04-02 10:00:00.000  1  1 I Tag     : x"]);
    app.following = false;
    handle_normal_key(&mut app, KeyCode::Char('p'));
    assert_eq!(app.detail, DetailView::Fields);
    handle_normal_key(&mut app, KeyCode::Esc);
    assert_eq!(app.detail, DetailView::Closed);
    assert!(!app.following, "Esc must not resume_following");
    assert_eq!(app.focus, app::Focus::LogList);
}

#[test]
fn test_detail_c_field_still_works() {
    let mut app = App::new(100);

    drain_lines(
        &mut app,
        &[
            "04-02 10:00:00.000  1  1 I Keep    : a",
            "04-02 10:00:01.000  1  1 I Drop    : b",
        ],
    );
    app.following = false;
    app.cursor = 0;
    handle_normal_key(&mut app, KeyCode::Char('p'));
    handle_normal_key(&mut app, KeyCode::Char('c'));
    handle_normal_key(&mut app, KeyCode::Char('t'));
    assert_eq!(app.groups.groups.len(), 1);
    assert_eq!(app.index.visible.len(), 1);
    assert!(app.detail_open(), "c+t keeps detail open");
}

#[test]
fn test_P_opens_pretty_and_switches_with_fields() {
    use crate::app::DetailView;
    let mut app = App::new(100);

    drain_lines(
        &mut app,
        &["04-02 10:00:00.000  1  1 I Tag     : {\"a\":1,\"b\":[2,3]}"],
    );
    app.following = false;
    handle_normal_key(&mut app, KeyCode::Char('P'));
    assert_eq!(app.detail, DetailView::Pretty);
    let (text, ok) = ui::pretty_json_for_row(&app.current_row().unwrap());
    assert!(ok);
    assert!(text.contains('\n'), "pretty should indent");
    handle_normal_key(&mut app, KeyCode::Char('P'));
    assert_eq!(app.detail, DetailView::Fields);
    handle_normal_key(&mut app, KeyCode::Char('P'));
    assert_eq!(app.detail, DetailView::Pretty);
    handle_normal_key(&mut app, KeyCode::Esc);
    assert_eq!(app.detail, DetailView::Closed);
    assert!(!app.following);
}

#[test]
fn test_pretty_non_json_shows_note() {
    let mut app = App::new(100);

    drain_lines(
        &mut app,
        &["04-02 10:00:00.000  1  1 I Tag     : not-json-at-all"],
    );
    app.following = false;
    handle_normal_key(&mut app, KeyCode::Char('P'));
    let lines = ui::detail_pretty_lines(app.current_row().as_deref(), 40);
    let joined: String = lines
        .iter()
        .map(|l| {
            l.spans
                .iter()
                .map(|s| s.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(joined.contains("not JSON"));
    assert!(joined.contains("not-json-at-all"));
}

#[test]
fn test_p_closes_pretty_mode() {
    use crate::app::DetailView;
    let mut app = App::new(100);

    drain_lines(
        &mut app,
        &["04-02 10:00:00.000  1  1 I Tag     : {\"x\":1}"],
    );
    handle_normal_key(&mut app, KeyCode::Char('P'));
    assert_eq!(app.detail, DetailView::Pretty);
    handle_normal_key(&mut app, KeyCode::Char('p'));
    assert_eq!(app.detail, DetailView::Closed);
}

#[test]
fn test_yc_yanks_cli_command() {
    let mut app = App::new(100);

    app.export_source = export::ExportSource::File("demo.log".into());
    drain_lines(&mut app, &["04-02 10:00:00.000  99  1 I MyTag   : hello"]);
    app.following = false;
    handle_normal_key(&mut app, KeyCode::Char('c'));
    handle_normal_key(&mut app, KeyCode::Char('t'));
    handle_normal_key(&mut app, KeyCode::Char('y'));
    assert_eq!(app.pending, Pending::Yank);
    handle_normal_key(&mut app, KeyCode::Char('c'));
    assert_ne!(app.pending, Pending::Yank);
    let cmd = app.last_yanked.as_deref().unwrap();
    assert!(cmd.starts_with("alnav grep -f 'demo.log' -i -e "));
    assert!(cmd.contains(r#"tag ~ "MyTag""#), "{cmd}");
}
