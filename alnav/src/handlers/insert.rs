//! Insert-mode (standalone Input box) key handling.

use crate::app::App;
use crate::handlers::{focus_loglist, focus_loglist_and_follow};
use crate::{app, input};
use crossterm::event::KeyCode;

pub(crate) fn handle_insert_key(
    app: &mut App,
    input: &mut input::InputBox,
    code: KeyCode,
) -> Result<(), String> {
    // Field candidates are draft-driven (no `/` open). Align with Search:
    // Up/Down move selection; Enter/Tab confirm when candidates exist.
    // Tab/BackTab cycle focus when there are no field candidates.
    // Enter two-step (no candidates): pending draft → commit pill; chips ready
    // → submit group; empty input → jump focus to LogList.
    // `/` and Space are literal draft chars. Filter chips always ignore-case.
    // Note: legacy Insert modal has no KeyEvent modifiers here — Ctrl chords
    // are unavailable; arrows/Home/End still edit the draft caret.
    match code {
        KeyCode::Esc => {
            *input = input::InputBox::default();
            app.mode = app::Mode::Normal;
            focus_loglist(app);
        }
        KeyCode::Up => {
            if input.field_popup_visible() {
                input.move_field_selection(-1);
            }
        }
        KeyCode::Down => {
            if input.field_popup_visible() {
                input.move_field_selection(1);
            }
        }
        KeyCode::Tab => {
            if !input.confirm_field_candidate() {
                app.cycle_focus_forward();
                app.mode = if app.focus == app::Focus::Input {
                    app::Mode::Insert
                } else {
                    app::Mode::Normal
                };
            }
        }
        KeyCode::BackTab => {
            app.cycle_focus_backward();
            app.mode = if app.focus == app::Focus::Input {
                app::Mode::Insert
            } else {
                app::Mode::Normal
            };
        }
        KeyCode::Enter => {
            if input.confirm_field_candidate_on_enter() {
                // picked field; draft cleared
            } else if input.has_pending_draft() {
                input.commit_draft_as_chip();
            } else if input.is_empty() {
                app.mode = app::Mode::Normal;
                app.focus = app::Focus::LogList;
            } else if input.exclude_mode {
                // H9: all pills become global excludes (not a Filter group).
                let chips = std::mem::take(&mut input.chips);
                let mut any = false;
                for chip in chips {
                    if app.push_exclude_chip(chip) {
                        any = true;
                    }
                }
                input.exclude_mode = false;
                if !any && app.status_msg.is_none() {
                    app.set_flash("EXISTS");
                }
                app.mode = app::Mode::Normal;
                focus_loglist_and_follow(app);
            } else if let Some(group) = input.build_group(true)? {
                if app.push_filter_group(group) {
                    app.rebuild_visible();
                }
                app.mode = app::Mode::Normal;
                focus_loglist_and_follow(app);
            }
        }
        KeyCode::Backspace => input.backspace(),
        KeyCode::Left => input.draft.move_left(),
        KeyCode::Right => input.draft.move_right(),
        KeyCode::Home => input.draft.home(),
        KeyCode::End => input.draft.end(),
        KeyCode::Char('!') if input.is_empty() => {
            if !input.toggle_exclude_mode() {
                input.push_char('!');
            }
        }
        KeyCode::Char(c) => input.push_char(c),
        _ => {}
    }
    Ok(())
}
