//! Key/mouse handlers shared by the event loop, with per-surface submodules.

use crate::app::App;
use crate::app::Pending;
use crate::handlers::normal::handle_normal_key;
use crate::{app, help, input, keymap, ui};
use crossterm::event::{self, KeyCode};
use ratatui::layout::Rect;

mod insert;
mod normal;
mod panels;
mod picker;
mod source;
#[cfg(test)]
mod tests;

pub(crate) use insert::handle_insert_key;
pub(crate) use panels::{
    handle_compare_key, handle_help_key, handle_highlight_box_key, handle_hist_panel_key,
    handle_palette_key, handle_preset_name_key, handle_summary_key, handle_time_panel_key,
};
pub(crate) use picker::keys::handle_picker_key;
pub(crate) use picker::render_data::picker_render_data;
pub(crate) use picker::submit::handle_confirm_key;
pub(crate) use source::{
    handle_dashboard_key, handle_open_file_panel_key, handle_stream_source_panel_key,
};

#[inline]
fn km_code(app: &App, id: keymap::ActionId, code: KeyCode) -> bool {
    app.keymap.matches_code(id, code)
}

#[inline]
fn km_event(app: &App, id: keymap::ActionId, key: event::KeyEvent) -> bool {
    app.keymap.matches_event(id, key)
}

/// Anchors the candidate dropdown just below the centered Input/Search modal.
pub(crate) fn popup_rect(modal: Rect, frame_area: Rect, match_count: usize) -> Rect {
    ui::candidate_popup_rect(modal, frame_area, match_count)
}

#[allow(dead_code)]
fn copy_to_clipboard(text: &str) -> Result<(), String> {
    arboard::Clipboard::new()
        .and_then(|mut cb| cb.set_text(text.to_string()))
        .map_err(|e| e.to_string())
}

fn apply_yank(app: &mut App, text: String) {
    app.apply_yank(text);
}

pub(crate) fn handle_ctrl_c(app: &mut App, input: &mut input::InputBox) {
    use app::Mode;

    match app.pending {
        Pending::Mark => return app.cancel_bookmark_op(),
        Pending::Chip | Pending::Exclude => return app.cancel_chip_from_cursor(),
        Pending::Lock => return app.cancel_lock_pending(),
        Pending::Time => return app.cancel_time_pending(),
        _ => {}
    }

    match app.mode {
        Mode::Normal => app.should_quit = true,
        Mode::Insert => {
            // Field popup is draft-driven; cancel always resets Input like Esc.
            *input = input::InputBox::default();
            app.mode = Mode::Normal;
            focus_loglist(app);
        }
    }
}

/// Return keyboard focus to the log list without changing cursor / offset /
/// following. Used when leaving ChipStrip/HighlightStrip/Input/HighlightBox.
fn focus_loglist(app: &mut App) {
    app.focus = app::Focus::LogList;
}

/// Return focus to the log list and resume live follow (pin to bottom).
/// Reserved for Esc on LogList, Visual Esc, and successful filter-group submit.
fn focus_loglist_and_follow(app: &mut App) {
    app.focus = app::Focus::LogList;
    app.resume_following();
}

/// Keep the retired standalone Input path testable until its remaining model
/// and renderer coverage can be removed; production entry points use Picker.
#[cfg(test)]
fn focus_input_insert(app: &mut App) {
    app.focus = app::Focus::Input;
    app.mode = app::Mode::Insert;
}

/// Mouse wheel step size for the log list, independent of
/// `action::PAGE_SIZE` (Ctrl-d/Ctrl-u) and the `Shift+J`/`Shift+K` fast-move binding — a wheel
/// "notch" is a smaller, more frequent unit than either of those.
const MOUSE_SCROLL_STEP: isize = 3;

/// LogList / Help `J`/`K` step — keep in sync via `help::FAST_SCROLL_STEP`.
const FAST_SCROLL_STEP: isize = help::FAST_SCROLL_STEP;

/// Only scroll wheel events move the log list; clicks/drags/moves are
/// ignored in this pass (no click-to-focus yet — see the design doc's
/// "Mouse wheel scrolling" section for why that's out of scope here). The
/// wheel always targets the log list regardless of which region currently
/// has keyboard focus. While visual-line is active, scrolling extends the
/// selection the same way `j`/`k` do.
pub(crate) fn handle_mouse_event(app: &mut App, mouse: event::MouseEvent) {
    match mouse.kind {
        event::MouseEventKind::ScrollDown => app.move_cursor_manual(MOUSE_SCROLL_STEP),
        event::MouseEventKind::ScrollUp => app.move_cursor_manual(-MOUSE_SCROLL_STEP),
        _ => {}
    }
}

/// Global Ctrl chords (palette, presets, open file/stream). True if handled.
fn dispatch_global_chords(app: &mut App, key: event::KeyEvent) -> bool {
    use keymap::ActionId;
    for id in [
        ActionId::GlobalCommandPalette,
        ActionId::LeaderPresetSave,
        ActionId::LeaderPresetOpen,
        ActionId::OpenFile,
        ActionId::OpenStream,
    ] {
        if km_event(app, id, key) {
            crate::action::dispatch(app, id);
            return true;
        }
    }
    false
}

/// Ctrl chords (command palette, paging, live-clear, source/preset) need the full KeyEvent.
pub(crate) fn handle_normal_event(
    app: &mut App,
    input: &mut input::InputBox,
    key: event::KeyEvent,
) {
    if dispatch_global_chords(app, key) {
        return;
    }
    if app.focus == app::Focus::LogList {
        if km_event(app, keymap::ActionId::LogListPageDown, key) {
            crate::action::dispatch(app, keymap::ActionId::LogListPageDown);
            return;
        }
        if km_event(app, keymap::ActionId::LogListPageUp, key) {
            crate::action::dispatch(app, keymap::ActionId::LogListPageUp);
            return;
        }
        if km_event(app, keymap::ActionId::LogListClearLive, key) {
            crate::action::dispatch(app, keymap::ActionId::LogListClearLive);
            return;
        }
    }
    handle_normal_key(app, input, key.code);
}
