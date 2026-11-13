//! Unified / Filter / Exclude / message picker and picker render-data tests.

use super::*;
use crate::app::App;
use crate::app::Pending;
use crate::handlers::normal::handle_normal_key;
use crate::handlers::picker::keys::handle_picker_key;
use crate::handlers::picker::render_data::{picker_render_data, unified_picker_items};
use crate::handlers::picker::submit::{handle_confirm_key, replace_last_token};
use crate::{app, input, keymap};
use crossterm::event::{self, KeyCode};
use crossterm::event::{KeyEvent, KeyModifiers};

#[test]
fn space_f_no_longer_opens_filter_picker() {
    let mut app = App::new(100);

    handle_normal_key(&mut app, KeyCode::Char(' '));
    assert_eq!(app.pending, Pending::Leader);
    handle_normal_key(&mut app, KeyCode::Char('f'));
    assert_ne!(app.pending, Pending::Leader);
    assert!(app.picker.is_none());
}

#[test]
fn space_space_opens_unified_manage() {
    use crate::picker::{PickerKind, PickerMode};

    let mut app = App::new(100);

    handle_normal_key(&mut app, KeyCode::Char(' '));
    assert_eq!(app.pending, Pending::Leader);
    handle_normal_key(&mut app, KeyCode::Char(' '));
    assert_ne!(app.pending, Pending::Leader);
    let picker = app.picker.as_ref().expect("unified picker");
    assert_eq!(picker.kind, PickerKind::Unified);
    assert_eq!(picker.mode, PickerMode::Manage);
}

#[test]
fn bare_keys_open_unified_or_new() {
    use crate::filter_model::Group;
    use crate::highlight_model::HighlightGroup;
    use crate::picker::{PickerKind, PickerMode};

    let mut app = App::new(100);

    app.groups.groups.push(Group {
        label: "g".into(),
        chips: Vec::new(),
        enabled: true,
        same_field_op: crate::fuzzy::SameFieldOp::And,
    });
    app.highlight_groups
        .groups
        .push(HighlightGroup::from_pattern("err").unwrap());
    assert!(app
        .groups
        .push_exclude(crate::input::Chip {
            field: crate::input::ChipField::Tag,
            value: "noise".into(),
        })
        .unwrap());

    handle_normal_key(&mut app, KeyCode::Char(';'));
    let filter_new = app.picker.as_ref().expect("; opens filter New");
    assert_eq!(filter_new.kind, PickerKind::Filter);
    assert_eq!(filter_new.mode, PickerMode::New);
    app.close_picker();

    handle_normal_key(&mut app, KeyCode::Char('/'));
    let hl = app.picker.as_ref().unwrap();
    assert_eq!(hl.kind, PickerKind::Highlight);
    assert_eq!(hl.mode, PickerMode::Manage);
    app.close_picker();

    handle_normal_key(&mut app, KeyCode::Char('`'));
    let exclude = app.picker.as_ref().expect("` opens exclude New");
    assert_eq!(exclude.kind, PickerKind::Exclude);
    assert_eq!(exclude.mode, PickerMode::New);
    app.close_picker();

    handle_normal_key(&mut app, KeyCode::Char('m'));
    assert_eq!(app.pending, Pending::Mark);
    handle_normal_key(&mut app, KeyCode::Char('m'));
    assert_ne!(app.pending, Pending::Mark);
    assert!(app.picker.is_none(), "mm must not open the Bookmark picker");
    assert!(app.compare.is_none());
    assert_eq!(app.status_msg.as_deref(), Some("NO BOOKMARKS"));
}

#[test]
fn manage_no_match_stays_in_manage() {
    use crate::highlight_model::HighlightGroup;
    use crate::picker::{PickerKind, PickerMode};

    let mut app = App::new(100);
    app.highlight_groups
        .groups
        .push(HighlightGroup::from_pattern("error").unwrap());
    app.open_unified_picker();
    assert_eq!(app.picker.as_ref().unwrap().mode, PickerMode::Manage);
    assert_eq!(app.picker.as_ref().unwrap().kind, PickerKind::Unified);

    handle_picker_key(
        &mut app,
        KeyEvent::new(KeyCode::Char('z'), KeyModifiers::NONE),
    );
    let picker = app.picker.as_ref().unwrap();
    assert_eq!(picker.mode, PickerMode::Manage);
    assert_eq!(picker.query, "z");
    assert!(picker.draft.is_empty());
}

#[test]
fn manual_new_clears_draft_stays_in_new() {
    use crate::picker::{PickerKind, PickerMode};

    let mut app = App::new(100);

    handle_normal_key(&mut app, KeyCode::Char(';'));
    assert_eq!(app.picker.as_ref().unwrap().mode, PickerMode::New);

    handle_picker_key(
        &mut app,
        KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE),
    );
    handle_picker_key(
        &mut app,
        KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE),
    );
    let picker = app.picker.as_ref().unwrap();
    assert_eq!(picker.mode, PickerMode::New);
    assert_eq!(picker.kind, PickerKind::Filter);
    assert!(picker.input.as_ref().is_some_and(|box_| box_.is_empty()));
}

#[test]
fn open_picker_new_forces_new_mode() {
    use crate::picker::{PickerKind, PickerMode};

    for kind in [
        PickerKind::Filter,
        PickerKind::Highlight,
        PickerKind::Exclude,
    ] {
        let mut app = App::new(100);
        app.following = false;
        app.open_picker_new(kind.clone());
        assert_eq!(
            app.picker.as_ref().unwrap().mode,
            PickerMode::New,
            "{kind:?} must open New"
        );
        handle_picker_key(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(app.picker.is_none(), "{kind:?} Esc from New closes");
        assert!(!app.following);
    }
}

#[test]
fn unified_picker_opens_in_manage_even_when_empty() {
    use crate::picker::{PickerKind, PickerMode};

    let mut app = App::new(100);
    app.open_unified_picker();
    assert_eq!(app.picker.as_ref().unwrap().kind, PickerKind::Unified);
    assert_eq!(app.picker.as_ref().unwrap().mode, PickerMode::Manage);
}

#[test]
fn picker_esc_closes_without_resuming_following() {
    use crate::picker::PickerKind;

    let mut app = App::new(100);
    app.following = false;
    app.open_picker_new(PickerKind::Highlight);
    handle_picker_key(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(app.picker.is_none());
    assert!(!app.following);
}

#[test]
fn unified_enter_toggles_highlight_without_jump() {
    use crate::highlight_model::HighlightGroup;

    let mut app = App::new(100);
    drain_lines(
        &mut app,
        &[
            "04-02 10:00:00.000  1  1 I T   : aaa",
            "04-02 10:00:00.000  1  1 I T   : error here",
        ],
    );
    app.following = false;
    app.cursor = 0;
    app.push_or_find_highlight_group(HighlightGroup::from_pattern("error").unwrap());
    let cursor_before = app.cursor;
    let active_before = app.active_highlight;
    app.open_unified_picker();
    handle_picker_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(app.picker.is_some());
    assert!(!app.highlight_groups.groups[0].enabled);
    assert_eq!(app.active_highlight, active_before);
    assert_eq!(app.cursor, cursor_before);
}

#[test]
fn picker_esc_from_unified_closes() {
    let mut app = App::new(100);
    app.open_unified_picker();
    handle_picker_key(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(app.picker.is_none());
    assert_eq!(app.focus, app::Focus::LogList);
}

#[test]
fn unified_delete_requires_confirm_and_can_cancel() {
    use crate::highlight_model::HighlightGroup;

    let mut app = App::new(100);
    app.push_or_find_highlight_group(HighlightGroup::from_pattern("error").unwrap());
    app.open_unified_picker();

    handle_picker_key(&mut app, KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE));
    assert!(app.confirm.is_some());
    handle_picker_key(
        &mut app,
        KeyEvent::new(KeyCode::Char('z'), KeyModifiers::NONE),
    );
    assert!(
        app.picker.as_ref().unwrap().query.is_empty(),
        "confirm state must swallow keys instead of editing the picker"
    );
    assert!(app.confirm.is_some());
    handle_picker_key(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(app.highlight_groups.groups.len(), 1);
    assert!(app.confirm.is_none());

    handle_picker_key(
        &mut app,
        KeyEvent::new(KeyCode::Backspace, KeyModifiers::CONTROL),
    );
    assert!(
        app.confirm.is_none(),
        "Ctrl-Backspace must not delete in Manage"
    );

    handle_picker_key(
        &mut app,
        KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL),
    );
    handle_picker_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(app.highlight_groups.groups.is_empty());
    assert!(app.picker.is_some());
}

#[test]
fn unified_enter_toggles_enabled_and_stays_open() {
    use crate::filter_model::Group;

    let mut app = App::new(100);
    app.groups.groups.push(Group {
        label: "first".into(),
        chips: Vec::new(),
        enabled: true,
        same_field_op: crate::fuzzy::SameFieldOp::And,
    });
    app.groups.groups.push(Group {
        label: "second".into(),
        chips: Vec::new(),
        enabled: true,
        same_field_op: crate::fuzzy::SameFieldOp::And,
    });
    app.open_unified_picker();
    handle_picker_key(&mut app, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    handle_picker_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert!(app.picker.is_some());
    assert!(app.groups.groups[0].enabled);
    assert!(!app.groups.groups[1].enabled);
}

#[test]
fn unified_tab_multiselect_enter_batch_toggles_enabled() {
    use crate::filter_model::Group;
    use crate::highlight_model::HighlightGroup;

    let mut app = App::new(100);
    app.groups.groups.push(Group {
        label: "f1".into(),
        chips: Vec::new(),
        enabled: true,
        same_field_op: crate::fuzzy::SameFieldOp::And,
    });
    app.push_or_find_highlight_group(HighlightGroup::from_pattern("h1").unwrap());
    app.open_unified_picker();

    // Tab marks first item and moves to second
    handle_picker_key(&mut app, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(app.picker.as_ref().unwrap().checked.len(), 1);
    assert_eq!(app.picker.as_ref().unwrap().selected, 1);
    // Tab marks second
    handle_picker_key(&mut app, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(app.picker.as_ref().unwrap().checked.len(), 2);

    handle_picker_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(app.confirm.is_none());
    assert!(!app.groups.groups[0].enabled);
    assert!(!app.highlight_groups.groups[0].enabled);
    assert_eq!(app.groups.groups.len(), 1);
    assert_eq!(app.highlight_groups.groups.len(), 1);
}

#[test]
fn unified_ctrl_t_disables_all_then_enables_all() {
    use crate::filter_model::{ExcludeEntry, Group};
    use crate::highlight_model::HighlightGroup;
    use crate::input::{Chip, ChipField};

    let mut app = App::new(100);
    app.groups.groups.push(Group {
        label: "f1".into(),
        chips: Vec::new(),
        enabled: true,
        same_field_op: crate::fuzzy::SameFieldOp::And,
    });
    app.groups.groups.push(Group {
        label: "f2".into(),
        chips: Vec::new(),
        enabled: false,
        same_field_op: crate::fuzzy::SameFieldOp::And,
    });
    app.push_or_find_highlight_group(HighlightGroup::from_pattern("h1").unwrap());
    app.groups.excludes.push(ExcludeEntry {
        chip: Chip {
            field: ChipField::Tag,
            value: "noise".into(),
        },
        enabled: true,
    });
    app.open_unified_picker();

    // Mixed state → disable everything.
    handle_picker_key(
        &mut app,
        KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL),
    );
    assert!(app.picker.is_some(), "panel stays open");
    assert!(app.groups.groups.iter().all(|g| !g.enabled));
    assert!(app.highlight_groups.groups.iter().all(|g| !g.enabled));
    assert!(app.groups.excludes.iter().all(|e| !e.enabled));

    // All disabled → enable everything.
    handle_picker_key(
        &mut app,
        KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL),
    );
    assert!(app.groups.groups.iter().all(|g| g.enabled));
    assert!(app.highlight_groups.groups.iter().all(|g| g.enabled));
    assert!(app.groups.excludes.iter().all(|e| e.enabled));
}

#[test]
fn unified_ctrl_t_scopes_to_checked_when_present() {
    use crate::filter_model::Group;
    use crate::highlight_model::HighlightGroup;

    let mut app = App::new(100);
    app.groups.groups.push(Group {
        label: "f1".into(),
        chips: Vec::new(),
        enabled: true,
        same_field_op: crate::fuzzy::SameFieldOp::And,
    });
    app.push_or_find_highlight_group(HighlightGroup::from_pattern("h1").unwrap());
    app.open_unified_picker();

    // Check only the first row (Tab also advances the cursor).
    handle_picker_key(&mut app, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(app.picker.as_ref().unwrap().checked.len(), 1);

    handle_picker_key(
        &mut app,
        KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL),
    );
    assert!(!app.groups.groups[0].enabled);
    assert!(
        app.highlight_groups.groups[0].enabled,
        "unchecked rows stay untouched"
    );
    assert_eq!(
        app.picker.as_ref().unwrap().checked.len(),
        1,
        "check set survives"
    );
}

#[test]
fn unified_ctrl_t_scopes_to_query_visible_rows() {
    use crate::filter_model::Group;
    use crate::highlight_model::HighlightGroup;

    let mut app = App::new(100);
    app.groups.groups.push(Group {
        label: "f1".into(),
        chips: Vec::new(),
        enabled: true,
        same_field_op: crate::fuzzy::SameFieldOp::And,
    });
    app.push_or_find_highlight_group(HighlightGroup::from_pattern("h1").unwrap());
    app.open_unified_picker();

    // Query keeps only the Highlight row visible.
    for c in "Highlight".chars() {
        handle_picker_key(
            &mut app,
            KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE),
        );
    }
    handle_picker_key(
        &mut app,
        KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL),
    );
    assert!(!app.highlight_groups.groups[0].enabled);
    assert!(app.groups.groups[0].enabled, "filtered-out rows untouched");
}

#[test]
fn unified_ctrl_k_clears_all_rules_after_confirm() {
    use crate::filter_model::{ExcludeEntry, Group};
    use crate::highlight_model::HighlightGroup;
    use crate::input::{Chip, ChipField};

    let mut app = App::new(100);
    app.groups.groups.push(Group {
        label: "f1".into(),
        chips: Vec::new(),
        enabled: true,
        same_field_op: crate::fuzzy::SameFieldOp::And,
    });
    app.push_or_find_highlight_group(HighlightGroup::from_pattern("h1").unwrap());
    app.groups.excludes.push(ExcludeEntry {
        chip: Chip {
            field: ChipField::Tag,
            value: "noise".into(),
        },
        enabled: true,
    });
    app.lock_pid = Some("9".into());
    app.time_bound = Some(crate::filter_model::TimeBound {
        since: Some("12:00:00".into()),
        until: None,
    });
    app.following = true;
    app.open_unified_picker();

    handle_picker_key(
        &mut app,
        KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL),
    );
    assert!(matches!(
        app.confirm,
        Some(crate::picker::ConfirmKind::ClearAll)
    ));
    assert_eq!(app.groups.groups.len(), 1);

    handle_picker_key(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(app.confirm.is_none());
    assert_eq!(app.groups.groups.len(), 1);
    assert!(app.picker.is_some());

    handle_picker_key(
        &mut app,
        KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL),
    );
    handle_picker_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(app.groups.groups.is_empty());
    assert!(app.groups.excludes.is_empty());
    assert!(app.highlight_groups.groups.is_empty());
    assert!(app.picker.is_none());
    assert_eq!(app.focus, app::Focus::LogList);
    assert!(!app.following);
    assert_eq!(app.lock_pid.as_deref(), Some("9"));
    assert!(app.time_bound.is_some());
    assert_eq!(app.status_msg.as_deref(), Some("ALL CLEARED"));
}

#[test]
fn unified_ctrl_k_empty_flashes_no_rules() {
    let mut app = App::new(100);
    app.open_unified_picker();
    handle_picker_key(
        &mut app,
        KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL),
    );
    assert!(app.confirm.is_none());
    assert!(app.picker.is_some());
    assert_eq!(app.status_msg.as_deref(), Some("NO RULES"));
}

#[test]
fn dispatch_clear_all_rules_confirms_without_opening_picker() {
    use crate::highlight_model::HighlightGroup;

    let mut app = App::new(100);
    app.push_or_find_highlight_group(HighlightGroup::from_pattern("h1").unwrap());
    crate::action::dispatch(&mut app, keymap::ActionId::ClearAllRules);
    assert!(app.picker.is_none());
    assert!(matches!(
        app.confirm,
        Some(crate::picker::ConfirmKind::ClearAll)
    ));
    handle_confirm_key(
        &mut app,
        KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE),
    );
    assert!(app.highlight_groups.groups.is_empty());
    assert!(app.confirm.is_none());
    assert_eq!(app.focus, app::Focus::LogList);
}

#[test]
fn exclude_picker_rejects_second_chip_draft() {
    use crate::input::{Chip, ChipField};
    use crate::picker::{PickerKind, PickerMode};

    let mut app = App::new(100);
    app.open_picker_new(PickerKind::Exclude);
    assert_eq!(app.picker.as_ref().unwrap().mode, PickerMode::New);
    app.picker
        .as_mut()
        .unwrap()
        .input
        .as_mut()
        .unwrap()
        .chips
        .push(Chip {
            field: ChipField::Tag,
            value: "first".into(),
        });

    handle_picker_key(
        &mut app,
        KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE),
    );

    let input = app.picker.as_ref().unwrap().input.as_ref().unwrap();
    assert_eq!(input.chips.len(), 1);
    assert!(input.draft.is_empty(), "second chip draft must be rejected");
}

#[test]
fn picker_query_mid_cursor_backspace_and_ctrl_u() {
    let mut app = App::new(100);
    app.open_unified_picker();
    for c in "abcdef".chars() {
        handle_picker_key(
            &mut app,
            KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE),
        );
    }
    handle_picker_key(&mut app, KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
    handle_picker_key(&mut app, KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
    handle_picker_key(
        &mut app,
        KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE),
    );
    assert_eq!(app.picker.as_ref().unwrap().query.as_str(), "abcef");
    handle_picker_key(
        &mut app,
        KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL),
    );
    assert_eq!(app.picker.as_ref().unwrap().query.as_str(), "ef");
    assert_eq!(app.picker.as_ref().unwrap().query.cursor(), 0);
}

#[test]
fn picker_new_ctrl_backspace_kills_word() {
    use crate::picker::PickerMode;

    let mut app = App::new(100);
    app.open_picker_new(crate::picker::PickerKind::Highlight);
    assert!(matches!(app.picker.as_ref().unwrap().mode, PickerMode::New));
    for c in "foo bar".chars() {
        handle_picker_key(
            &mut app,
            KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE),
        );
    }
    handle_picker_key(
        &mut app,
        KeyEvent::new(KeyCode::Backspace, KeyModifiers::CONTROL),
    );
    assert_eq!(app.picker.as_ref().unwrap().draft.as_str(), "foo ");
}

#[test]
fn picker_ctrl_a_homes_without_mutating_text() {
    let mut app = App::new(100);
    app.open_unified_picker();
    for c in "abc".chars() {
        handle_picker_key(
            &mut app,
            KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE),
        );
    }
    handle_picker_key(
        &mut app,
        KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL),
    );
    let q = &app.picker.as_ref().unwrap().query;
    assert_eq!(q.as_str(), "abc", "Ctrl-A must not insert characters");
    assert_eq!(q.cursor(), 0);
}

#[test]
fn picker_arrows_move_caret_without_mutating_text() {
    let mut app = App::new(100);
    app.open_unified_picker();
    for c in "abc".chars() {
        handle_picker_key(
            &mut app,
            KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE),
        );
    }
    handle_picker_key(&mut app, KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
    handle_picker_key(&mut app, KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
    let q = &app.picker.as_ref().unwrap().query;
    assert_eq!(q.as_str(), "abc");
    assert_eq!(q.cursor(), 1);
    handle_picker_key(&mut app, KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
    assert_eq!(app.picker.as_ref().unwrap().query.as_str(), "abc");
    assert_eq!(app.picker.as_ref().unwrap().query.cursor(), 2);
}

#[test]
fn manage_ctrl_e_no_longer_edits() {
    use crate::highlight_model::HighlightGroup;
    use crate::picker::PickerMode;

    let mut app = App::new(100);
    app.push_or_find_highlight_group(HighlightGroup::from_pattern("error").unwrap());
    app.open_unified_picker();
    handle_picker_key(
        &mut app,
        KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL),
    );
    // Ctrl-E is end-of-line on the (empty) query, not edit.
    assert!(matches!(
        app.picker.as_ref().unwrap().mode,
        PickerMode::Manage
    ));
    assert_eq!(app.picker.as_ref().unwrap().query.cursor(), 0);
}

#[test]
fn test_semicolon_opens_filter_picker_in_new() {
    use crate::picker::{PickerKind, PickerMode};

    let mut app = App::new(100);

    handle_normal_key(&mut app, KeyCode::Char(';'));
    let picker = app.picker.as_ref().expect("; must open filter New");
    assert_eq!(picker.kind, PickerKind::Filter);
    assert_eq!(picker.mode, PickerMode::New);

    assert_eq!(app.focus, app::Focus::LogList);
}

#[test]
fn test_backtick_opens_exclude_picker() {
    use crate::picker::{PickerKind, PickerMode};

    let mut app = App::new(100);

    handle_normal_key(&mut app, KeyCode::Char('`'));
    let picker = app.picker.as_ref().expect("` must open exclude picker");
    assert_eq!(picker.kind, PickerKind::Exclude);
    // Empty excludes → New (same rule as Filter/Highlight/Bookmark).
    assert_eq!(picker.mode, PickerMode::New);

    assert_eq!(app.focus, app::Focus::LogList);
}

#[test]
fn filter_picker_enter_confirms_field_candidate_without_pill() {
    use crate::picker::{PickerKind, PickerMode};

    let mut app = App::new(100);
    app.open_picker_new(PickerKind::Filter);
    for c in "tag".chars() {
        handle_picker_key(
            &mut app,
            event::KeyEvent::new(KeyCode::Char(c), event::KeyModifiers::NONE),
        );
    }
    handle_picker_key(
        &mut app,
        event::KeyEvent::new(KeyCode::Enter, event::KeyModifiers::NONE),
    );
    let input = app
        .picker
        .as_ref()
        .and_then(|session| session.input.as_ref())
        .expect("filter new keeps InputBox");
    assert_eq!(input.draft_field, Some(input::ChipField::Tag));
    assert!(input.draft.is_empty());
    assert!(
        input.chips.is_empty(),
        "field confirm must not commit a pill"
    );
    assert!(matches!(
        app.picker.as_ref().map(|session| &session.mode),
        Some(PickerMode::New)
    ));
    let data = picker_render_data(&app, 10, 80).unwrap();
    assert_eq!(data.draft_field, Some(input::ChipField::Tag));
    assert!(data.text.is_empty());
}

#[test]
fn filter_picker_tab_confirms_field_candidate() {
    use crate::picker::PickerKind;

    let mut app = App::new(100);
    app.open_picker_new(PickerKind::Filter);
    handle_picker_key(
        &mut app,
        event::KeyEvent::new(KeyCode::Char('t'), event::KeyModifiers::NONE),
    );
    handle_picker_key(
        &mut app,
        event::KeyEvent::new(KeyCode::Down, event::KeyModifiers::NONE),
    ); // tid
    handle_picker_key(
        &mut app,
        event::KeyEvent::new(KeyCode::Tab, event::KeyModifiers::NONE),
    );
    let input = app
        .picker
        .as_ref()
        .and_then(|session| session.input.as_ref())
        .unwrap();
    assert_eq!(input.draft_field, Some(input::ChipField::Tid));
    assert!(input.draft.is_empty());
}

#[test]
fn test_cm_opens_picker_enter_pushes_token() {
    use crate::picker::{MsgChipPurpose, PickerKind, PickerMode};

    let mut app = App::new(100);

    drain_lines(
        &mut app,
        &[
            "04-02 10:00:00.000  1  1 I Tag     : hello timeout world",
            "04-02 10:00:01.000  1  1 I Tag     : other",
        ],
    );
    app.following = false;
    app.cursor = 0;
    handle_normal_key(&mut app, KeyCode::Char('c'));
    handle_normal_key(&mut app, KeyCode::Char('m'));
    assert!(matches!(
        app.picker
            .as_ref()
            .map(|picker| (&picker.kind, &picker.mode)),
        Some((
            PickerKind::MsgChip {
                purpose: MsgChipPurpose::Chip { exclude: false }
            },
            PickerMode::New
        ))
    ));
    // filter to "timeout"
    handle_picker_key(
        &mut app,
        event::KeyEvent::new(KeyCode::Char('t'), event::KeyModifiers::NONE),
    );
    handle_picker_key(
        &mut app,
        event::KeyEvent::new(KeyCode::Enter, event::KeyModifiers::NONE),
    );
    assert!(matches!(
        app.picker.as_ref().map(|p| &p.kind),
        Some(PickerKind::ActionList { value }) if value == "timeout"
    ));
    assert_eq!(app.picker.as_ref().unwrap().selected, 0);
    handle_picker_key(
        &mut app,
        event::KeyEvent::new(KeyCode::Enter, event::KeyModifiers::NONE),
    );
    assert!(app.picker.is_none());
    assert_eq!(app.groups.groups.len(), 1);
    assert_eq!(app.groups.groups[0].chips[0].value, "timeout");
    assert_eq!(app.index.visible.len(), 1);
}

#[test]
fn test_cm_draft_fallback_when_no_candidate() {
    let mut app = App::new(100);

    drain_lines(
        &mut app,
        &[
            "04-02 10:00:00.000  1  1 I Tag     : hello world",
            "04-02 10:00:01.000  1  1 I Tag     : customxyz",
        ],
    );
    app.following = false;
    app.cursor = 0;
    handle_normal_key(&mut app, KeyCode::Char('c'));
    handle_normal_key(&mut app, KeyCode::Char('m'));
    for c in "customxyz".chars() {
        handle_picker_key(
            &mut app,
            event::KeyEvent::new(KeyCode::Char(c), event::KeyModifiers::NONE),
        );
    }
    assert!(picker_render_data(&app, 10, 80).unwrap().labels.is_empty());
    handle_picker_key(
        &mut app,
        event::KeyEvent::new(KeyCode::Enter, event::KeyModifiers::NONE),
    );
    assert!(matches!(
        app.picker.as_ref().map(|p| &p.kind),
        Some(crate::picker::PickerKind::ActionList { value }) if value == "customxyz"
    ));
    handle_picker_key(
        &mut app,
        event::KeyEvent::new(KeyCode::Enter, event::KeyModifiers::NONE),
    );
    assert_eq!(app.groups.groups[0].chips[0].value, "customxyz");
    assert_eq!(app.index.visible.len(), 1);
}

#[test]
fn test_cm_picker_esc_closes_instead_of_opening_manage_mode() {
    let mut app = App::new(100);

    drain_lines(
        &mut app,
        &["04-02 10:00:00.000  1  1 I Tag     : hello world"],
    );
    handle_normal_key(&mut app, KeyCode::Char('c'));
    handle_normal_key(&mut app, KeyCode::Char('m'));
    handle_picker_key(
        &mut app,
        event::KeyEvent::new(KeyCode::Esc, event::KeyModifiers::NONE),
    );
    assert!(app.picker.is_none());
}

#[test]
fn test_capital_cm_picker_pushes_exclude_token() {
    use crate::picker::PickerKind;

    let mut app = App::new(100);

    drain_lines(
        &mut app,
        &[
            "04-02 10:00:00.000  1  1 I Tag     : noisy timeout",
            "04-02 10:00:01.000  1  1 I Tag     : useful",
        ],
    );
    app.cursor = 0;
    handle_normal_key(&mut app, KeyCode::Char('C'));
    handle_normal_key(&mut app, KeyCode::Char('m'));
    assert!(matches!(
        app.picker.as_ref().map(|picker| &picker.kind),
        Some(PickerKind::MsgChip {
            purpose: crate::picker::MsgChipPurpose::Chip { exclude: true }
        })
    ));
    for c in "timeout".chars() {
        handle_picker_key(
            &mut app,
            event::KeyEvent::new(KeyCode::Char(c), event::KeyModifiers::NONE),
        );
    }
    handle_picker_key(
        &mut app,
        event::KeyEvent::new(KeyCode::Enter, event::KeyModifiers::NONE),
    );
    assert!(app.picker.is_none(), "Cm skips ActionList");
    assert_eq!(app.groups.excludes.len(), 1);
    assert_eq!(app.groups.excludes[0].chip.value, "timeout");
    assert_eq!(app.index.visible.len(), 1);
}

#[test]
fn test_cm_action_list_highlight() {
    let mut app = App::new(100);

    drain_lines(
        &mut app,
        &[
            "04-02 10:00:00.000  1  1 I Tag     : hello timeout world",
            "04-02 10:00:01.000  1  1 I Tag     : timeout again",
        ],
    );
    app.following = false;
    app.cursor = 0;
    handle_normal_key(&mut app, KeyCode::Char('c'));
    handle_normal_key(&mut app, KeyCode::Char('m'));
    for c in "timeout".chars() {
        handle_picker_key(
            &mut app,
            event::KeyEvent::new(KeyCode::Char(c), event::KeyModifiers::NONE),
        );
    }
    handle_picker_key(
        &mut app,
        event::KeyEvent::new(KeyCode::Enter, event::KeyModifiers::NONE),
    );
    // Input starts empty (not prefilled with the msg token).
    let data = picker_render_data(&app, 10, 80).unwrap();
    assert_eq!(data.title, "Create");
    assert!(data.text.is_empty(), "query must not carry msg token");
    assert_eq!(data.labels, vec!["Filter", "Highlight"]);
    // Substring search (not fuzzy): type "h" → only Highlight.
    handle_picker_key(
        &mut app,
        event::KeyEvent::new(KeyCode::Char('h'), event::KeyModifiers::NONE),
    );
    let data = picker_render_data(&app, 10, 80).unwrap();
    assert_eq!(data.text, "h");
    assert_eq!(data.labels, vec!["Highlight"]);
    handle_picker_key(
        &mut app,
        event::KeyEvent::new(KeyCode::Enter, event::KeyModifiers::NONE),
    );
    assert!(app.picker.is_none());
    assert!(app.groups.groups.is_empty());
    assert_eq!(app.highlight_groups.groups.len(), 1);
    assert_eq!(app.highlight_groups.groups[0].pattern, "timeout");
    assert_eq!(app.active_highlight, Some(0));
}

#[test]
fn test_cm_action_list_esc_cancels() {
    let mut app = App::new(100);

    drain_lines(
        &mut app,
        &["04-02 10:00:00.000  1  1 I Tag     : hello timeout"],
    );
    app.following = false;
    handle_normal_key(&mut app, KeyCode::Char('c'));
    handle_normal_key(&mut app, KeyCode::Char('m'));
    for c in "timeout".chars() {
        handle_picker_key(
            &mut app,
            event::KeyEvent::new(KeyCode::Char(c), event::KeyModifiers::NONE),
        );
    }
    handle_picker_key(
        &mut app,
        event::KeyEvent::new(KeyCode::Enter, event::KeyModifiers::NONE),
    );
    assert!(matches!(
        app.picker.as_ref().map(|p| &p.kind),
        Some(crate::picker::PickerKind::ActionList { .. })
    ));
    handle_picker_key(
        &mut app,
        event::KeyEvent::new(KeyCode::Esc, event::KeyModifiers::NONE),
    );
    assert!(app.picker.is_none());
    assert!(app.groups.groups.is_empty());
    assert!(app.highlight_groups.groups.is_empty());
    assert!(!app.following);
}

#[test]
fn test_msg_token_candidates_uncapped() {
    let msg = "aa bb cc dd ee ff gg hh ii jj kk";
    let tokens = input::msg_token_candidates(msg);
    assert!(tokens.len() > 8, "got {}", tokens.len());
    assert!(tokens.contains(&"kk".to_string()));
}

#[test]
fn unified_picker_has_no_bookmark_items() {
    // AC2/F2: the aggregated panel no longer lists bookmarks.
    let mut app = App::new(100);
    drain_lines(&mut app, &["04-02 10:00:00.000  1  1 I Tag     : x"]);
    app.cursor = 0;
    app.bookmark_add_current();
    let items = unified_picker_items(&app);
    // No item label carries a bookmark-style prefix, and the bookmark
    // still exists in the session (it just isn't surfaced in the panel).
    assert!(items.iter().all(|i| !i.label.starts_with("[Bookmark]")));
    assert!(items.iter().all(
        |i| i.id.kind != crate::picker::UnifiedKind::Filter || i.label.starts_with("[Filter]")
    ));
    assert_eq!(
        app.bookmarks.len(),
        1,
        "bookmark still exists outside panel"
    );
}

#[test]
fn test_leader_space_opens_unified_picker() {
    use crate::picker::PickerKind;

    let mut app = App::new(100);

    drain_lines(&mut app, &["04-02 10:00:00.000  1  1 I Tag     : x"]);
    handle_normal_key(&mut app, KeyCode::Char(' '));
    handle_normal_key(&mut app, KeyCode::Char(' '));
    assert_eq!(
        app.picker.as_ref().map(|picker| &picker.kind),
        Some(&PickerKind::Unified)
    );
}

#[test]
fn filter_picker_new_submit_returns_focus_to_loglist() {
    use crate::input::{Chip, ChipField};
    use crate::picker::{PickerKind, PickerMode};

    let mut app = App::new(100);
    app.focus = app::Focus::ChipStrip;
    app.open_picker_new(PickerKind::Filter);
    assert_eq!(app.picker.as_ref().unwrap().mode, PickerMode::New);
    let input = app.picker.as_mut().unwrap().input.as_mut().unwrap();
    input.chips.push(Chip {
        field: ChipField::Tag,
        value: "Foo".into(),
    });
    handle_picker_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert_eq!(app.groups.groups.len(), 1);
    assert!(app.picker.is_none());
    assert_eq!(app.focus, app::Focus::LogList);
}

#[test]
fn exclude_picker_new_submit_returns_focus_to_loglist() {
    use crate::input::{Chip, ChipField};
    use crate::picker::{PickerKind, PickerMode};

    let mut app = App::new(100);
    app.focus = app::Focus::ExcludeStrip;
    app.open_picker_new(PickerKind::Exclude);
    assert_eq!(app.picker.as_ref().unwrap().mode, PickerMode::New);
    let input = app.picker.as_mut().unwrap().input.as_mut().unwrap();
    input.chips.push(Chip {
        field: ChipField::Tag,
        value: "noise".into(),
    });
    handle_picker_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert_eq!(app.groups.excludes.len(), 1);
    assert!(app.picker.is_none());
    assert_eq!(app.focus, app::Focus::LogList);
}

#[test]
fn picker_render_data_filter_shows_tag_vocab_after_field_pick() {
    use std::sync::mpsc;
    let mut app = App::new(100);
    let (tx, rx) = mpsc::channel();
    tx.send(crate::model::EntryRow::from_line("01-01 00:00:00.000  1 1 I TargetTag: msg").unwrap())
        .unwrap();
    drop(tx);
    app.drain(&rx);

    app.open_picker_new(crate::picker::PickerKind::Filter);
    {
        let input = app.picker.as_mut().unwrap().input.as_mut().unwrap();
        input.set_field(crate::input::ChipField::Tag);
    }
    app.flush_vocab_match();

    let data = picker_render_data(&app, 10, 80).unwrap();
    assert!(
        data.labels.iter().any(|l| l == "TargetTag"),
        "Tag vocab should appear in labels after field pick, got: {:?}",
        data.labels
    );
}

#[test]
fn picker_render_data_filter_level_static_candidates() {
    let mut app = App::new(100);
    app.open_picker_new(crate::picker::PickerKind::Filter);
    {
        let input = app.picker.as_mut().unwrap().input.as_mut().unwrap();
        input.set_field(crate::input::ChipField::Level);
    }
    let data = picker_render_data(&app, 10, 80).unwrap();
    assert!(data.labels.contains(&"W".to_string()));
    assert!(data.labels.contains(&"E".to_string()));
}

#[test]
fn picker_render_data_unified_never_shows_preview() {
    use crate::picker::PickerKind;
    let mut app = App::new(100);
    app.open_picker(PickerKind::Unified);
    let data = picker_render_data(&app, 10, 80).unwrap();
    assert!(
        !data.show_preview,
        "Unified picker must never render preview"
    );
    assert!(data.preview.is_empty());
}

#[test]
fn picker_render_data_filter_new_empty_draft_has_empty_preview() {
    use crate::picker::PickerKind;
    let mut app = App::new(100);
    app.open_picker_new(PickerKind::Filter);
    let data = picker_render_data(&app, 10, 80).unwrap();
    // draft_field == None, draft empty: field keyword candidates shown, preview empty
    assert!(data.draft_field.is_none());
    assert!(data.preview.is_empty());
    assert!(data.show_preview, "Filter picker still shows preview pane");
    // field keywords should appear immediately without typing
    assert!(data.labels.iter().any(|l| l == "tag"));
}

#[test]
fn picker_render_data_filter_new_field_set_empty_draft_has_empty_preview() {
    use crate::input::ChipField;
    use crate::picker::PickerKind;
    use std::sync::mpsc;
    let mut app = App::new(100);
    let (tx, rx) = mpsc::channel();
    tx.send(crate::model::EntryRow::from_line("01-01 00:00:00.000  1 1 I TargetTag: msg").unwrap())
        .unwrap();
    drop(tx);
    app.drain(&rx);
    app.open_picker_new(PickerKind::Filter);
    {
        let input = app.picker.as_mut().unwrap().input.as_mut().unwrap();
        input.set_field(ChipField::Tag);
    }
    // field set but draft still empty: no preview content yet
    let data = picker_render_data(&app, 10, 80).unwrap();
    assert!(data.draft_field == Some(ChipField::Tag));
    assert!(
        data.preview.is_empty(),
        "empty draft value must not preview"
    );
}

#[test]
fn replace_last_token_middle() {
    assert_eq!(replace_last_token("MyApp tim", "timeout"), "MyApp timeout");
}

#[test]
fn replace_last_token_no_space() {
    assert_eq!(replace_last_token("timeout", "error"), "error");
}

#[test]
fn picker_tab_fills_tag_vocab_candidate() {
    use std::sync::mpsc;
    let (tx, rx) = mpsc::channel();
    let mut app = App::new(100);
    let row =
        crate::model::EntryRow::from_line("01-01 00:00:00.000  1 1 I TabTestTag: msg").unwrap();
    tx.send(row).unwrap();
    drop(tx);
    app.drain(&rx);

    app.open_picker_new(crate::picker::PickerKind::Filter);
    {
        let input = app.picker.as_mut().unwrap().input.as_mut().unwrap();
        input.set_field(crate::input::ChipField::Tag);
        input.push_char('T');
    }
    app.flush_vocab_match();

    handle_picker_key(
        &mut app,
        crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Tab,
            crossterm::event::KeyModifiers::NONE,
        ),
    );

    let draft = app
        .picker
        .as_ref()
        .unwrap()
        .input
        .as_ref()
        .unwrap()
        .draft
        .clone();
    assert_eq!(
        draft, "TabTestTag",
        "Tab should fill vocab candidate into draft"
    );
}
