//! Help panel, command palette and summary panel key tests.

use crate::app::App;
use crate::handlers::normal::handle_normal_key;
use crate::handlers::panels::{handle_help_key, handle_palette_key, handle_summary_key};
use crate::handlers::{handle_normal_event, FAST_SCROLL_STEP};
use crate::{app, input};
use crossterm::event::KeyCode;
use crossterm::event::{KeyEvent, KeyModifiers};

#[test]
fn question_opens_help_and_esc_closes_without_follow() {
    let mut app = App::new(100);
    let mut input = input::InputBox::default();
    app.following = false;
    handle_normal_key(&mut app, &mut input, KeyCode::Char('?'));
    assert!(app.help_open);
    handle_help_key(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(!app.help_open);
    assert!(!app.following, "closing help must not resume following");
}

#[test]
fn help_shift_jk_scrolls_by_fast_step() {
    let mut app = App::new(100);
    let mut input = input::InputBox::default();
    handle_normal_key(&mut app, &mut input, KeyCode::Char('?'));
    assert!(app.help_open);
    handle_help_key(
        &mut app,
        KeyEvent::new(KeyCode::Char('4'), KeyModifiers::NONE),
    );
    let body_len = crate::help::page_doc_lines(&app, crate::help::HelpPage::Log).len();
    assert!(
        body_len > FAST_SCROLL_STEP as usize,
        "log help page needs more than {FAST_SCROLL_STEP} lines, got {body_len}"
    );
    assert!(matches!(
        app.help_view,
        crate::help::HelpView::Page {
            id: crate::help::HelpPage::Log,
            scroll: 0
        }
    ));
    handle_help_key(
        &mut app,
        KeyEvent::new(KeyCode::Char('J'), KeyModifiers::NONE),
    );
    assert!(matches!(
        app.help_view,
        crate::help::HelpView::Page {
            id: crate::help::HelpPage::Log,
            scroll: 7
        }
    ));
    handle_help_key(
        &mut app,
        KeyEvent::new(KeyCode::Char('K'), KeyModifiers::NONE),
    );
    assert!(matches!(
        app.help_view,
        crate::help::HelpView::Page {
            id: crate::help::HelpPage::Log,
            scroll: 0
        }
    ));
}

#[test]
fn help_jk_scrolls_one_line_and_question_closes_without_follow() {
    let mut app = App::new(100);
    let mut input = input::InputBox::default();
    app.following = false;
    handle_normal_key(&mut app, &mut input, KeyCode::Char('?'));
    assert!(app.help_open);
    handle_help_key(
        &mut app,
        KeyEvent::new(KeyCode::Char('4'), KeyModifiers::NONE),
    );
    handle_help_key(
        &mut app,
        KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE),
    );
    assert!(matches!(
        app.help_view,
        crate::help::HelpView::Page {
            id: crate::help::HelpPage::Log,
            scroll: 1
        }
    ));
    handle_help_key(
        &mut app,
        KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE),
    );
    assert!(matches!(
        app.help_view,
        crate::help::HelpView::Page {
            id: crate::help::HelpPage::Log,
            scroll: 0
        }
    ));
    handle_help_key(
        &mut app,
        KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE),
    );
    assert!(!app.help_open);
    assert!(
        !app.following,
        "closing help with ? must not resume following"
    );
}

#[test]
fn help_h_backs_to_home_esc_closes() {
    let mut app = App::new(100);
    let mut input = input::InputBox::default();
    handle_normal_key(&mut app, &mut input, KeyCode::Char('?'));
    handle_help_key(
        &mut app,
        KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE),
    );
    assert!(matches!(
        app.help_view,
        crate::help::HelpView::Page {
            id: crate::help::HelpPage::Exclude,
            ..
        }
    ));
    handle_help_key(
        &mut app,
        KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE),
    );
    assert!(
        matches!(app.help_view, crate::help::HelpView::Home { toc: 1, .. }),
        "{:?}",
        app.help_view
    );
    assert!(app.help_open);
    handle_help_key(
        &mut app,
        KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE),
    );
    handle_help_key(
        &mut app,
        KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE),
    );
    assert!(
        matches!(app.help_view, crate::help::HelpView::Home { toc: 1, .. }),
        "{:?}",
        app.help_view
    );
    handle_help_key(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(!app.help_open);
}

#[test]
fn help_home_h_is_noop() {
    let mut app = App::new(100);
    let mut input = input::InputBox::default();
    handle_normal_key(&mut app, &mut input, KeyCode::Char('?'));
    handle_help_key(
        &mut app,
        KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE),
    );
    assert!(app.help_open);
    assert!(matches!(
        app.help_view,
        crate::help::HelpView::Home { toc: 3, .. }
    ));
}

#[test]
fn help_digit_jumps_across_pages() {
    let mut app = App::new(100);
    let mut input = input::InputBox::default();
    handle_normal_key(&mut app, &mut input, KeyCode::Char('?'));
    handle_help_key(
        &mut app,
        KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE),
    );
    assert!(matches!(
        app.help_view,
        crate::help::HelpView::Page {
            id: crate::help::HelpPage::Filter,
            ..
        }
    ));
    handle_help_key(
        &mut app,
        KeyEvent::new(KeyCode::Char('7'), KeyModifiers::NONE),
    );
    assert!(matches!(
        app.help_view,
        crate::help::HelpView::Page {
            id: crate::help::HelpPage::Overlays,
            scroll: 0
        }
    ));
    handle_help_key(
        &mut app,
        KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE),
    );
    assert!(matches!(
        app.help_view,
        crate::help::HelpView::Page {
            id: crate::help::HelpPage::Overlays,
            scroll: 1
        }
    ));
}

#[test]
fn help_enter_opens_preselected_exclude() {
    let mut app = App::new(100);
    let mut input = input::InputBox::default();
    app.focus = app::Focus::ExcludeStrip;
    handle_normal_key(&mut app, &mut input, KeyCode::Char('?'));
    assert!(matches!(
        app.help_view,
        crate::help::HelpView::Home { toc: 1, .. }
    ));
    handle_help_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(matches!(
        app.help_view,
        crate::help::HelpView::Page {
            id: crate::help::HelpPage::Exclude,
            ..
        }
    ));
}

#[test]
fn help_search_prompt_k_types_and_esc_clears() {
    let mut app = App::new(100);
    let mut input = input::InputBox::default();
    handle_normal_key(&mut app, &mut input, KeyCode::Char('?'));
    handle_help_key(
        &mut app,
        KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE),
    );
    assert!(app.help_search_prompting());
    assert!(
        app.picker.is_none(),
        "Help / must not open Highlight picker"
    );
    handle_help_key(
        &mut app,
        KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE),
    );
    assert_eq!(
        app.help_search.as_ref().map(|s| s.query.as_str()),
        Some("k")
    );
    handle_help_key(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(app.help_open);
    assert!(app.help_search.is_none());
    handle_help_key(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(!app.help_open);
}

#[test]
fn help_search_empty_enter_clears() {
    let mut app = App::new(100);
    let mut input = input::InputBox::default();
    handle_normal_key(&mut app, &mut input, KeyCode::Char('?'));
    handle_help_key(
        &mut app,
        KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE),
    );
    handle_help_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(app.help_open);
    assert!(app.help_search.is_none());
}

#[test]
fn help_search_n_walks_hits_and_ctrl_c_closes() {
    let mut app = App::new(100);
    let mut input = input::InputBox::default();
    app.following = false;
    handle_normal_key(&mut app, &mut input, KeyCode::Char('?'));
    handle_help_key(
        &mut app,
        KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE),
    );
    for c in ['c', 'h', 'i', 'p'] {
        handle_help_key(
            &mut app,
            KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE),
        );
    }
    handle_help_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    let search = app.help_search.as_ref().expect("committed search");
    assert!(!search.prompt);
    assert!(search.hits.len() >= 2, "chip should hit more than once");
    let first = search.current;
    let first_hit = search.hits[first].clone();
    handle_help_key(
        &mut app,
        KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE),
    );
    let search = app.help_search.as_ref().expect("hits kept");
    assert_ne!(search.current, first);
    assert_ne!(search.hits[search.current], first_hit);
    handle_help_key(
        &mut app,
        KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
    );
    assert!(!app.help_open);
    assert!(!app.following);
    assert!(app.help_search.is_none());
}

#[test]
fn help_search_no_match_flashes() {
    let mut app = App::new(100);
    let mut input = input::InputBox::default();
    handle_normal_key(&mut app, &mut input, KeyCode::Char('?'));
    let view = app.help_view.clone();
    handle_help_key(
        &mut app,
        KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE),
    );
    for c in ['~', '~', '~'] {
        handle_help_key(
            &mut app,
            KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE),
        );
    }
    assert_eq!(app.status_msg.as_deref(), Some("NO MATCH"));
    assert!(app.help_open);
    assert_eq!(app.help_view, view);
}

#[test]
fn question_ignored_while_operator_pending() {
    let mut app = App::new(100);
    let mut input = input::InputBox::default();
    app.pending_yank = true;
    handle_normal_key(&mut app, &mut input, KeyCode::Char('?'));
    assert!(!app.help_open);
}

#[test]
fn ctrl_p_opens_empty_command_palette() {
    let mut app = App::new(100);
    let mut input = input::InputBox::default();
    app.following = true;
    handle_normal_event(
        &mut app,
        &mut input,
        KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL),
    );
    let pal = app.command_palette.as_ref().expect("palette");
    assert!(pal.query.is_empty());
    assert!(!app.following);
    assert_eq!(app.focus, app::Focus::LogList);
}

#[test]
fn colon_does_not_open_command_palette() {
    let mut app = App::new(100);
    let mut input = input::InputBox::default();
    handle_normal_event(
        &mut app,
        &mut input,
        KeyEvent::new(KeyCode::Char(':'), KeyModifiers::NONE),
    );
    assert!(app.command_palette.is_none());
}

#[test]
fn pending_chip_then_ctrl_p_opens_palette() {
    let mut app = App::new(100);
    let mut input = input::InputBox::default();
    handle_normal_key(&mut app, &mut input, KeyCode::Char('c'));
    assert!(app.pending_chip);
    handle_normal_event(
        &mut app,
        &mut input,
        KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL),
    );
    assert!(app.command_palette.is_some());
    assert!(!app.pending_chip);
    assert!(app.command_palette.as_ref().unwrap().query.is_empty());
}

#[test]
fn ctrl_p_ignored_when_detail_open() {
    let mut app = App::new(100);
    let mut input = input::InputBox::default();
    app.toggle_detail_fields();
    assert!(app.detail_open());
    handle_normal_event(
        &mut app,
        &mut input,
        KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL),
    );
    assert!(
        app.command_palette.is_none(),
        "R3: palette must not open over Detail"
    );
    assert!(app.detail_open());
}

#[test]
fn palette_esc_closes_without_resume_following() {
    let mut app = App::new(100);
    app.following = false;
    app.open_command_palette();
    handle_palette_key(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(app.command_palette.is_none());
    assert!(!app.following);
}

#[test]
fn palette_ctrl_c_closes_without_resume_following() {
    let mut app = App::new(100);
    app.following = false;
    app.open_command_palette();
    handle_palette_key(
        &mut app,
        KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
    );
    assert!(app.command_palette.is_none());
    assert!(!app.following);
}

#[test]
fn palette_typing_bookmark_k_does_not_move_selection() {
    let mut app = App::new(100);
    app.open_command_palette();
    for c in ['b', 'o', 'o', 'k', 'm', 'a', 'r', 'k'] {
        handle_palette_key(
            &mut app,
            KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE),
        );
    }
    let pal = app.command_palette.as_ref().unwrap();
    assert_eq!(pal.query.as_str(), "bookmark");
    assert_eq!(pal.selected, 0, "k must type, not move Down");
}

#[test]
fn palette_zero_match_enter_is_noop() {
    let mut app = App::new(100);
    app.open_command_palette();
    for c in "zzzznotacommand".chars() {
        handle_palette_key(
            &mut app,
            KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE),
        );
    }
    handle_palette_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(app.command_palette.is_some());
    assert!(app.picker.is_none());
    assert!(!app.should_quit);
}

#[test]
fn palette_enter_filter_opens_filter_new() {
    let mut app = App::new(100);
    app.open_command_palette();
    for c in "filter".chars() {
        handle_palette_key(
            &mut app,
            KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE),
        );
    }
    handle_palette_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(app.command_palette.is_none());
    let picker = app.picker.as_ref().expect("filter picker");
    assert_eq!(picker.kind, crate::picker::PickerKind::Filter);
    assert!(matches!(picker.mode, crate::picker::PickerMode::New));
}

#[test]
fn space_i_opens_summary_panel_and_esc_closes_without_follow() {
    let mut app = App::new(100);
    let mut input = input::InputBox::default();
    app.following = false;
    handle_normal_key(&mut app, &mut input, KeyCode::Char(' '));
    assert!(app.pending_leader);
    handle_normal_key(&mut app, &mut input, KeyCode::Char('i'));
    assert!(!app.pending_leader);
    assert!(app.summary_open());

    handle_summary_key(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(!app.summary_open());
    assert!(!app.following, "Esc must not resume following");
}

#[test]
fn space_i_while_open_toggles_close() {
    let mut app = App::new(100);
    let mut input = input::InputBox::default();
    handle_normal_key(&mut app, &mut input, KeyCode::Char(' '));
    handle_normal_key(&mut app, &mut input, KeyCode::Char('i'));
    assert!(app.summary_open());

    handle_summary_key(
        &mut app,
        KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE),
    );
    assert!(app.pending_leader);
    handle_summary_key(
        &mut app,
        KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE),
    );
    assert!(!app.pending_leader);
    assert!(!app.summary_open(), "Leader i while open re-toggles closed");
}

#[test]
fn summary_jk_scrolls_ready_body() {
    let mut app = App::new(100);
    let mut input = input::InputBox::default();
    let (tx, rx) = std::sync::mpsc::channel();
    for i in 0..3 {
        tx.send(
            crate::model::EntryRow::from_line(&format!(
                "04-02 10:00:00.000  1  1 I Tag{i}   : line{i}"
            ))
            .unwrap(),
        )
        .unwrap();
    }
    drop(tx);
    app.drain(&rx);
    handle_normal_key(&mut app, &mut input, KeyCode::Char(' '));
    handle_normal_key(&mut app, &mut input, KeyCode::Char('i'));
    app.flush_summary_job(std::time::Duration::from_secs(5));
    assert!(matches!(app.summary_view, app::SummaryView::Ready(_)));

    assert_eq!(app.summary_scroll, 0);
    handle_summary_key(
        &mut app,
        KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE),
    );
    assert_eq!(app.summary_scroll, 1);
    handle_summary_key(
        &mut app,
        KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE),
    );
    assert_eq!(app.summary_scroll, 0);
}
