//! Insert-mode Input box key tests.

use super::*;
use crate::app::App;
use crate::handlers::insert::handle_insert_key;
use crate::handlers::normal::handle_normal_key;
use crate::handlers::{focus_input_insert, handle_ctrl_c};
use crate::{app, input};
use crossterm::event::KeyCode;

#[test]
fn test_ctrl_c_in_insert_mode_cancels_input_instead_of_quitting() {
    let mut app = App::new(100);
    let mut input = input::InputBox::default();
    focus_input_insert(&mut app);
    input.push_char('x');
    handle_ctrl_c(&mut app, &mut input);
    assert!(!app.should_quit);
    assert_eq!(app.mode, app::Mode::Normal);
    assert!(input.is_empty());
}

#[test]
fn test_ctrl_c_in_insert_mode_also_returns_focus_to_loglist() {
    let mut app = App::new(100);
    let mut input = input::InputBox::default();
    focus_input_insert(&mut app);
    input.push_char('x');
    handle_ctrl_c(&mut app, &mut input);
    assert_eq!(app.mode, app::Mode::Normal);
    assert_eq!(
        app.focus,
        app::Focus::LogList,
        "Ctrl+C should behave like Esc: also return focus to the log list"
    );
}

#[test]
fn test_ctrl_c_with_field_popup_visible_resets_input() {
    let mut app = App::new(100);
    let mut input = input::InputBox::default();
    focus_input_insert(&mut app);
    input.push_char('t'); // auto field popup visible
    assert!(input.field_popup_visible());
    handle_ctrl_c(&mut app, &mut input);
    assert!(input.is_empty());
    assert!(!input.field_popup_visible());
    assert_eq!(app.mode, app::Mode::Normal);
    assert_eq!(app.focus, app::Focus::LogList);
}

#[test]
fn test_slash_is_literal_in_input_draft() {
    let mut app = App::new(100);
    let mut input = input::InputBox::default();
    focus_input_insert(&mut app);
    handle_insert_key(&mut app, &mut input, KeyCode::Char('/')).unwrap();
    assert_eq!(input.draft, "/");
    assert!(input.field_popup_visible());
    assert!(
        input.field_candidates().is_empty(),
        "/ matches no field keyword"
    );
    assert!(input.draft_field.is_none());
}

#[test]
fn test_enter_with_field_candidates_confirms_field() {
    let mut app = App::new(100);
    let mut input = input::InputBox::default();
    focus_input_insert(&mut app);
    for c in "tag".chars() {
        handle_insert_key(&mut app, &mut input, KeyCode::Char(c)).unwrap();
    }
    handle_insert_key(&mut app, &mut input, KeyCode::Enter).unwrap();
    assert_eq!(input.draft_field, Some(input::ChipField::Tag));
    assert!(input.draft.is_empty());
    assert!(
        input.chips.is_empty(),
        "confirming a field must not commit a pill"
    );
    assert!(!input.field_popup_visible());
}

#[test]
fn test_tab_with_field_candidates_confirms_field() {
    let mut app = App::new(100);
    let mut input = input::InputBox::default();
    focus_input_insert(&mut app);
    handle_insert_key(&mut app, &mut input, KeyCode::Char('t')).unwrap();
    handle_insert_key(&mut app, &mut input, KeyCode::Down).unwrap(); // tid
    handle_insert_key(&mut app, &mut input, KeyCode::Tab).unwrap();
    assert_eq!(input.draft_field, Some(input::ChipField::Tid));
    assert_eq!(
        app.focus,
        app::Focus::Input,
        "Tab must not cycle focus when confirming a field"
    );
    assert_eq!(app.mode, app::Mode::Insert);
}

#[test]
fn test_enter_without_field_candidates_commits_pill() {
    let mut app = App::new(100);
    let mut input = input::InputBox::default();
    focus_input_insert(&mut app);
    for c in "error".chars() {
        handle_insert_key(&mut app, &mut input, KeyCode::Char(c)).unwrap();
    }
    assert!(input.field_popup_visible());
    assert!(input.field_candidates().is_empty());
    handle_insert_key(&mut app, &mut input, KeyCode::Enter).unwrap();
    assert_eq!(input.chips.len(), 1);
    assert_eq!(input.chips[0].field, input::ChipField::Msg);
    assert_eq!(input.chips[0].value, "error");
    assert!(!input.field_popup_visible());
}

#[test]
fn test_esc_in_insert_mode_resets_input_and_returns_to_normal() {
    let mut app = App::new(100);
    let mut input = input::InputBox::default();
    focus_input_insert(&mut app);
    input.push_char('x');
    handle_insert_key(&mut app, &mut input, KeyCode::Esc).unwrap();
    assert_eq!(app.mode, app::Mode::Normal);
    assert_eq!(
        app.focus,
        app::Focus::LogList,
        "Esc should also return focus to the log list"
    );
    assert!(input.is_empty());
}

#[test]
fn test_enter_two_step_builds_group_and_returns_focus_to_loglist() {
    let mut app = App::new(100);
    let mut input = input::InputBox::default();
    focus_input_insert(&mut app);
    input.push_char('x');
    handle_insert_key(&mut app, &mut input, KeyCode::Enter).unwrap(); // commit pill
    assert_eq!(input.chips.len(), 1);
    assert_eq!(app.groups.groups.len(), 0);
    handle_insert_key(&mut app, &mut input, KeyCode::Enter).unwrap(); // submit group
    assert_eq!(app.groups.groups.len(), 1);
    assert_eq!(
        app.mode,
        app::Mode::Normal,
        "adding a group should behave like Esc: back to Normal"
    );
    assert_eq!(
        app.focus,
        app::Focus::LogList,
        "adding a group should jump focus back to the log list"
    );
    assert!(app.following);
    assert!(input.chips.is_empty());
}

#[test]
fn test_enter_with_empty_input_focuses_loglist() {
    let mut app = App::new(100);
    let mut input = input::InputBox::default();
    focus_input_insert(&mut app);
    handle_insert_key(&mut app, &mut input, KeyCode::Enter).unwrap();
    assert!(
        app.groups.groups.is_empty(),
        "empty draft must not build a group"
    );
    assert_eq!(app.mode, app::Mode::Normal);
    assert_eq!(
        app.focus,
        app::Focus::LogList,
        "empty Enter switches focus to LogList"
    );
}

#[test]
fn test_tab_in_insert_cycles_focus_and_normal_tab_cycles_visible_only() {
    use crate::filter_model::Group;
    use crate::highlight_model::HighlightGroup;
    use crate::input::{Chip, ChipField};

    // Insert Tab still cycles through every region (Input is a valid
    // editing target there); draft is preserved.
    let mut app = App::new(100);
    let mut input = input::InputBox::default();
    focus_input_insert(&mut app);
    input.push_char('x');
    handle_insert_key(&mut app, &mut input, KeyCode::Tab).unwrap();
    assert_eq!(app.focus, app::Focus::ChipStrip);
    assert_eq!(app.mode, app::Mode::Normal);
    assert_eq!(input.draft, "x");

    // Normal Tab with all strips empty: only LogList is visible, so Tab
    // stays on LogList and never opens the unified picker.
    handle_normal_key(&mut app, &mut input, KeyCode::Tab);
    assert_eq!(app.focus, app::Focus::LogList);
    assert!(app.picker.is_none());

    // Populate all three strips so they become visible.
    app.groups.groups.push(Group {
        label: "g".into(),
        chips: Vec::new(),
        enabled: true,
        same_field_op: crate::fuzzy::SameFieldOp::And,
    });
    assert!(app
        .groups
        .push_exclude(Chip {
            field: ChipField::Tag,
            value: "noise".into(),
        })
        .unwrap());
    app.highlight_groups
        .groups
        .push(HighlightGroup::from_pattern("err").unwrap());

    // From LogList, BackTab lands on HighlightStrip (last visible region).
    handle_normal_key(&mut app, &mut input, KeyCode::BackTab);
    assert_eq!(app.focus, app::Focus::HighlightStrip);
    // Forward cycle wraps to the head then advances:
    // Highlight → LogList → ChipStrip → ExcludeStrip → Highlight.
    handle_normal_key(&mut app, &mut input, KeyCode::Tab);
    assert_eq!(app.focus, app::Focus::LogList);
    handle_normal_key(&mut app, &mut input, KeyCode::Tab);
    assert_eq!(app.focus, app::Focus::ChipStrip);
    handle_normal_key(&mut app, &mut input, KeyCode::Tab);
    assert_eq!(app.focus, app::Focus::ExcludeStrip);
    handle_normal_key(&mut app, &mut input, KeyCode::Tab);
    assert_eq!(app.focus, app::Focus::HighlightStrip);
    // Never opens the unified picker via Tab.
    assert!(app.picker.is_none());
}

#[test]
fn test_digit_in_insert_is_literal_not_focus_switch() {
    let mut app = App::new(100);
    let mut input = input::InputBox::default();
    focus_input_insert(&mut app);
    handle_insert_key(&mut app, &mut input, KeyCode::Char('1')).unwrap();
    assert_eq!(app.focus, app::Focus::Input);
    assert_eq!(input.draft, "1");
}

#[test]
fn test_space_is_literal_in_input_draft() {
    let mut app = App::new(100);
    let mut input = input::InputBox::default();
    focus_input_insert(&mut app);
    input.push_char('a');
    handle_insert_key(&mut app, &mut input, KeyCode::Char(' ')).unwrap();
    input.push_char('b');
    assert_eq!(input.draft, "a b");
    assert!(input.chips.is_empty());
}

#[test]
fn test_filter_build_group_ignore_case_by_default() {
    let mut app = App::new(100);
    let mut input = input::InputBox::default();
    focus_input_insert(&mut app);
    input.set_field(input::ChipField::Tag);
    for c in "mytag".chars() {
        input.push_char(c);
    }
    handle_insert_key(&mut app, &mut input, KeyCode::Enter).unwrap(); // pill
    handle_insert_key(&mut app, &mut input, KeyCode::Enter).unwrap(); // submit
    let row = crate::model::EntryRow::from_line("04-02 10:00:00.000  1  1 I MyTag   : m").unwrap();
    assert!(
        app.groups.matches(&row),
        "filter chips must ignore case by default"
    );
}

#[test]
fn test_filter_group_dedup_skips_duplicate() {
    let mut app = App::new(100);
    let mut input = input::InputBox::default();
    focus_input_insert(&mut app);
    input.push_char('x');
    handle_insert_key(&mut app, &mut input, KeyCode::Enter).unwrap();
    handle_insert_key(&mut app, &mut input, KeyCode::Enter).unwrap();
    assert_eq!(app.groups.groups.len(), 1);
    focus_input_insert(&mut app);
    input.push_char('x');
    handle_insert_key(&mut app, &mut input, KeyCode::Enter).unwrap();
    handle_insert_key(&mut app, &mut input, KeyCode::Enter).unwrap();
    assert_eq!(
        app.groups.groups.len(),
        1,
        "duplicate filter group must not be added"
    );
}

#[test]
fn test_input_bang_exclude_mode_submit() {
    let mut app = App::new(100);
    let mut input = input::InputBox::default();
    drain_lines(
        &mut app,
        &[
            "04-02 10:00:00.000  1  1 I Noise   : a",
            "04-02 10:00:01.000  1  1 I Keep    : b",
        ],
    );
    focus_input_insert(&mut app);
    handle_insert_key(&mut app, &mut input, KeyCode::Char('!')).unwrap();
    assert!(input.exclude_mode);
    // pick tag field via draft prefix
    input.push_char('t');
    input.push_char('a');
    assert!(input.confirm_field_candidate());
    for c in "Noise".chars() {
        input.push_char(c);
    }
    handle_insert_key(&mut app, &mut input, KeyCode::Enter).unwrap(); // commit pill
    handle_insert_key(&mut app, &mut input, KeyCode::Enter).unwrap(); // submit excludes
    assert_eq!(app.groups.excludes.len(), 1);
    assert_eq!(app.visible.len(), 1);
    assert_eq!(
        app.view_source()[app.source_idx_for_visible(0).unwrap()].tag,
        "Keep"
    );
}
