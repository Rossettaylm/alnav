//! Highlight picker (New / Manage / auto-new / create-row) tests.

use super::*;
use crate::app::App;
use crate::handlers::normal::handle_normal_key;
use crate::handlers::picker::keys::handle_picker_key;
use crate::handlers::picker::render_data::picker_render_data;
use crate::{app, input, theme};
use crossterm::event::KeyCode;
use crossterm::event::{KeyEvent, KeyModifiers};

#[test]
fn slash_still_opens_highlight_new() {
    let mut app = App::new(100);
    let mut input = input::InputBox::default();
    handle_normal_key(&mut app, &mut input, KeyCode::Char('/'));
    let picker = app.picker.as_ref().expect("highlight picker");
    assert_eq!(picker.kind, crate::picker::PickerKind::Highlight);
    assert!(matches!(picker.mode, crate::picker::PickerMode::New));
}

#[test]
fn slash_opens_highlight_new() {
    use crate::picker::{PickerKind, PickerMode};

    let mut app = App::new(100);
    let mut input = input::InputBox::default();
    handle_normal_key(&mut app, &mut input, KeyCode::Char('/'));
    assert!(!app.highlight_box.editing);
    let picker = app.picker.as_ref().expect("highlight picker");
    assert_eq!(picker.kind, PickerKind::Highlight);
    assert_eq!(picker.mode, PickerMode::New);
}

#[test]
fn highlight_picker_new_submit_closes_picker() {
    use crate::picker::{PickerKind, PickerMode};

    let mut app = App::new(100);
    app.open_picker_new(PickerKind::Highlight);
    assert_eq!(app.picker.as_ref().unwrap().mode, PickerMode::New);
    for c in "error".chars() {
        handle_picker_key(
            &mut app,
            KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE),
        );
    }
    handle_picker_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert_eq!(app.highlight_groups.groups.len(), 1);
    assert_eq!(app.highlight_groups.groups[0].pattern, "error");
    assert!(app.picker.is_none());
    assert_eq!(app.active_highlight, Some(0));
    assert_eq!(app.focus, app::Focus::LogList);
}

#[test]
fn slash_opens_highlight_manage_when_groups_exist() {
    use crate::highlight_model::HighlightGroup;
    use crate::picker::{PickerKind, PickerMode};

    let mut app = App::new(100);
    let mut input = input::InputBox::default();
    app.push_or_find_highlight_group(HighlightGroup::from_pattern("error").unwrap());
    handle_normal_key(&mut app, &mut input, KeyCode::Char('/'));
    let picker = app.picker.as_ref().expect("finder");
    assert_eq!(picker.kind, PickerKind::Highlight);
    assert_eq!(picker.mode, PickerMode::Manage);
    let data = picker_render_data(&app, 10, 80).unwrap();
    assert_eq!(data.labels, vec!["error".to_string()]);
    assert!(!data.labels.iter().any(|l| l.contains("[Highlight]")));
}

#[test]
fn highlight_manage_enter_activates_enables_jumps_and_closes() {
    use crate::highlight_model::HighlightGroup;
    use crate::ui::ActionKind;

    let mut app = App::new(100);
    drain_lines(
        &mut app,
        &[
            "04-02 10:00:00.000  1  1 I T   : aaa",
            "04-02 10:00:00.000  1  1 I T   : error here",
            "04-02 10:00:00.000  1  1 I T   : warn too",
        ],
    );
    app.following = false;
    app.cursor = 0;
    app.push_or_find_highlight_group(HighlightGroup::from_pattern("error").unwrap());
    app.push_or_find_highlight_group(HighlightGroup::from_pattern("warn").unwrap());
    app.highlight_groups.groups[0].enabled = false;
    app.open_highlight_finder();
    let data = picker_render_data(&app, 10, 80).unwrap();
    assert!(matches!(data.actions.first(), Some(ActionKind::Jump)));
    handle_picker_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(app.picker.is_none());
    assert!(app.highlight_groups.groups[0].enabled);
    assert!(app.highlight_groups.groups[1].enabled);
    assert_eq!(app.active_highlight, Some(0));
    assert_eq!(app.cursor, 1);
    assert_eq!(app.focus, app::Focus::LogList);
    assert!(!app.view_focus.highlight);
}

#[test]
fn highlight_manage_auto_new_and_clear_returns() {
    use crate::highlight_model::HighlightGroup;
    use crate::picker::{PickerKind, PickerMode};

    let mut app = App::new(100);
    app.push_or_find_highlight_group(HighlightGroup::from_pattern("error").unwrap());
    app.open_highlight_finder();
    assert_eq!(app.picker.as_ref().unwrap().mode, PickerMode::Manage);
    for c in "zzz".chars() {
        handle_picker_key(
            &mut app,
            KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE),
        );
    }
    let picker = app.picker.as_ref().unwrap();
    assert_eq!(picker.mode, PickerMode::New);
    assert_eq!(picker.draft.as_str(), "zzz");
    assert!(picker.auto_from_manage);
    for _ in 0..3 {
        handle_picker_key(
            &mut app,
            KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE),
        );
    }
    let picker = app.picker.as_ref().unwrap();
    assert_eq!(picker.kind, PickerKind::Highlight);
    assert_eq!(picker.mode, PickerMode::Manage);
    assert!(picker.query.is_empty());
    assert!(!picker.auto_from_manage);
}

#[test]
fn highlight_new_er_does_not_steal_error() {
    use crate::highlight_model::HighlightGroup;
    use crate::picker::PickerKind;

    let mut app = App::new(100);
    app.push_or_find_highlight_group(HighlightGroup::from_pattern("error").unwrap());
    app.open_picker_new(PickerKind::Highlight);
    for c in "er".chars() {
        handle_picker_key(
            &mut app,
            KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE),
        );
    }
    handle_picker_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(app.highlight_groups.groups.len(), 2);
    assert_eq!(app.highlight_groups.groups[1].pattern, "er");
    assert_eq!(app.active_highlight, Some(1));
    assert!(app.picker.is_none());
}

#[test]
fn highlight_new_exact_ignore_case_reuses() {
    use crate::highlight_model::HighlightGroup;
    use crate::picker::PickerKind;

    let mut app = App::new(100);
    drain_lines(
        &mut app,
        &[
            "04-02 10:00:00.000  1  1 I T   : aaa",
            "04-02 10:00:00.000  1  1 I T   : ERROR boom",
        ],
    );
    app.following = false;
    app.cursor = 0;
    app.push_or_find_highlight_group(HighlightGroup::from_pattern("error").unwrap());
    app.open_picker_new(PickerKind::Highlight);
    for c in "ERROR".chars() {
        handle_picker_key(
            &mut app,
            KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE),
        );
    }
    handle_picker_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(app.highlight_groups.groups.len(), 1);
    assert_eq!(app.active_highlight, Some(0));
    assert_eq!(app.cursor, 1);
    assert!(app.picker.is_none());
}

#[test]
fn highlight_new_empty_draft_lists_no_history() {
    use crate::highlight_model::HighlightGroup;
    use crate::picker::PickerKind;

    let mut app = App::new(100);
    app.push_or_find_highlight_group(HighlightGroup::from_pattern("error").unwrap());
    app.open_picker_new(PickerKind::Highlight);
    let data = picker_render_data(&app, 10, 80).unwrap();
    assert!(data.labels.is_empty(), "empty New must not list history");
    assert_eq!(data.empty_msg, "type a pattern");
}

#[test]
fn highlight_manage_delete_last_opens_new() {
    use crate::highlight_model::HighlightGroup;
    use crate::picker::{PickerKind, PickerMode};

    let mut app = App::new(100);
    app.push_or_find_highlight_group(HighlightGroup::from_pattern("error").unwrap());
    app.open_highlight_finder();
    handle_picker_key(&mut app, KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE));
    handle_picker_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(app.highlight_groups.groups.is_empty());
    let picker = app.picker.as_ref().expect("stays open");
    assert_eq!(picker.kind, PickerKind::Highlight);
    assert_eq!(picker.mode, PickerMode::New);
    assert!(!picker.auto_from_manage);
}

#[test]
fn highlight_manage_delete_last_visible_auto_news() {
    use crate::highlight_model::HighlightGroup;
    use crate::picker::{PickerKind, PickerMode};

    let mut app = App::new(100);
    app.push_or_find_highlight_group(HighlightGroup::from_pattern("error").unwrap());
    app.push_or_find_highlight_group(HighlightGroup::from_pattern("warn").unwrap());
    app.open_highlight_finder();
    for c in "war".chars() {
        handle_picker_key(
            &mut app,
            KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE),
        );
    }
    assert_eq!(app.picker.as_ref().unwrap().mode, PickerMode::Manage);
    handle_picker_key(&mut app, KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE));
    handle_picker_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(app.highlight_groups.groups.len(), 1);
    assert_eq!(app.highlight_groups.groups[0].pattern, "error");
    let picker = app.picker.as_ref().unwrap();
    assert_eq!(picker.kind, PickerKind::Highlight);
    assert_eq!(picker.mode, PickerMode::New);
    assert_eq!(picker.draft.as_str(), "war");
    assert!(picker.auto_from_manage);
}

#[test]
fn highlight_manage_create_row_skips_prefix_hit() {
    use crate::highlight_model::HighlightGroup;
    use crate::picker::PickerMode;

    let mut app = App::new(100);
    app.push_or_find_highlight_group(HighlightGroup::from_pattern("unreadItem").unwrap());
    app.open_highlight_finder();
    for c in "unread".chars() {
        handle_picker_key(
            &mut app,
            KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE),
        );
    }
    assert_eq!(app.picker.as_ref().unwrap().mode, PickerMode::Manage);
    let data = picker_render_data(&app, 10, 80).unwrap();
    assert_eq!(
        data.labels,
        vec![
            "unreadItem".to_string(),
            format!("{} unread", theme::GLYPH_MODE_NEW),
        ]
    );
    handle_picker_key(&mut app, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    handle_picker_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(app.highlight_groups.groups.len(), 2);
    assert_eq!(app.highlight_groups.groups[1].pattern, "unread");
    assert_eq!(app.active_highlight, Some(1));
    assert!(app.picker.is_none());
}

#[test]
fn highlight_manage_exact_query_hides_create_row() {
    use crate::highlight_model::HighlightGroup;
    use crate::picker::PickerMode;

    let mut app = App::new(100);
    app.push_or_find_highlight_group(HighlightGroup::from_pattern("unreadItem").unwrap());
    app.open_highlight_finder();
    for c in "unreaditem".chars() {
        handle_picker_key(
            &mut app,
            KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE),
        );
    }
    assert_eq!(app.picker.as_ref().unwrap().mode, PickerMode::Manage);
    let data = picker_render_data(&app, 10, 80).unwrap();
    assert_eq!(data.labels, vec!["unreadItem".to_string()]);
    handle_picker_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(app.highlight_groups.groups.len(), 1);
    assert_eq!(app.active_highlight, Some(0));
    assert!(app.picker.is_none());
}

#[test]
fn highlight_manage_create_row_absent_when_query_empty() {
    use crate::highlight_model::HighlightGroup;

    let mut app = App::new(100);
    app.push_or_find_highlight_group(HighlightGroup::from_pattern("unreadItem").unwrap());
    app.open_highlight_finder();
    let data = picker_render_data(&app, 10, 80).unwrap();
    assert_eq!(data.labels, vec!["unreadItem".to_string()]);
    assert!(!data
        .labels
        .iter()
        .any(|l| l.contains(theme::GLYPH_MODE_NEW)));
}

#[test]
fn highlight_manage_enter_on_prefix_hit_still_activates() {
    use crate::highlight_model::HighlightGroup;
    use crate::picker::PickerMode;

    let mut app = App::new(100);
    app.push_or_find_highlight_group(HighlightGroup::from_pattern("unreadItem").unwrap());
    app.open_highlight_finder();
    for c in "unread".chars() {
        handle_picker_key(
            &mut app,
            KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE),
        );
    }
    assert_eq!(app.picker.as_ref().unwrap().mode, PickerMode::Manage);
    handle_picker_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(app.highlight_groups.groups.len(), 1);
    assert_eq!(app.active_highlight, Some(0));
    assert!(app.picker.is_none());
}

#[test]
fn highlight_new_reuses_disabled_and_jumps() {
    use crate::highlight_model::HighlightGroup;
    use crate::picker::PickerKind;

    let mut app = App::new(100);
    drain_lines(
        &mut app,
        &[
            "04-02 10:00:00.000  1  1 I T   : aaa",
            "04-02 10:00:00.000  1  1 I T   : ERROR boom",
        ],
    );
    app.following = false;
    app.cursor = 0;
    app.push_or_find_highlight_group(HighlightGroup::from_pattern("error").unwrap());
    app.highlight_groups.groups[0].enabled = false;
    app.open_picker_new(PickerKind::Highlight);
    for c in "ERROR".chars() {
        handle_picker_key(
            &mut app,
            KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE),
        );
    }
    handle_picker_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(app.highlight_groups.groups.len(), 1);
    assert!(app.highlight_groups.groups[0].enabled);
    assert_eq!(app.active_highlight, Some(0));
    assert_eq!(app.cursor, 1);
    assert!(app.picker.is_none());
    assert!(!app.view_focus.highlight);
}

#[test]
fn filtered_highlight_edit_submit_closes_and_updates_pattern() {
    use crate::highlight_model::HighlightGroup;
    use crate::picker::PickerMode;

    let mut app = App::new(100);
    for pattern in ["alpha", "error", "warn"] {
        app.push_or_find_highlight_group(HighlightGroup::from_pattern(pattern).unwrap());
    }
    app.open_unified_picker();
    for c in "warn".chars() {
        handle_picker_key(
            &mut app,
            KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE),
        );
    }
    handle_picker_key(
        &mut app,
        KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL),
    );
    assert_eq!(
        app.picker.as_ref().unwrap().mode,
        PickerMode::Edit { index: 2 }
    );
    handle_picker_key(
        &mut app,
        KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE),
    );
    handle_picker_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert!(app.picker.is_none());
    assert_eq!(app.highlight_groups.groups[2].pattern, "warni");
    assert_eq!(app.active_highlight, Some(2));
}
