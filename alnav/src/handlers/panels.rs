//! Overlay panel keys: highlight box, preset name, compare, help, summary, histogram, time, palette.

use crate::app::App;
use crate::handlers::picker::submit::apply_text_field_key;
use crate::handlers::{focus_loglist, km_code, km_event, FAST_SCROLL_STEP};
use crate::{app, keymap, time_panel};
use crossterm::event::{self, KeyCode};

/// Independent of `handle_normal_key`/`handle_insert_key`/`handle_ctrl_c`:
/// dispatched before any of them while `app.highlight_box.editing`. Ctrl+C
/// here cancels the draft (like Esc), not quit. Invalid regex on Enter is
/// silently ignored so a typo can't end the session or drop existing groups.
/// Enter/Tab with history candidates mirrors Input field-popup confirm.
pub(crate) fn handle_highlight_box_key(app: &mut App, key: event::KeyEvent) {
    let ctrl = key.modifiers.contains(event::KeyModifiers::CONTROL);
    let is_ctrl_c = key.code == KeyCode::Char('c') && ctrl;
    match key.code {
        KeyCode::Up => app
            .highlight_box
            .move_selection(&app.highlight_groups.groups, -1),
        KeyCode::Down => app
            .highlight_box
            .move_selection(&app.highlight_groups.groups, 1),
        KeyCode::Enter | KeyCode::Tab => {
            match app
                .highlight_box
                .confirm_or_submit(&app.highlight_groups.groups)
            {
                Ok(Some(group)) => {
                    let idx = app.push_or_find_highlight_group(group);
                    app.highlight_box.clear();
                    let _ = app.jump_first_match_of(idx);
                    app.focus = app::Focus::LogList;
                }
                Ok(None) => {
                    // empty Enter/Tab: no-op (stay editing)
                }
                Err(()) => {
                    // bad regex: exit editing, keep prior search groups
                    app.highlight_box.clear();
                    focus_loglist(app);
                }
            }
        }
        KeyCode::Esc => {
            app.highlight_box.clear();
            focus_loglist(app);
        }
        _ if is_ctrl_c => {
            app.highlight_box.clear();
            focus_loglist(app);
        }
        KeyCode::Backspace if ctrl => {
            app.highlight_box.draft.kill_word_back();
            app.highlight_box.selected = 0;
        }
        KeyCode::Delete => {}
        code => {
            if apply_text_field_key(&mut app.highlight_box.draft, code, ctrl) {
                app.highlight_box.selected = 0;
            }
        }
    }
}

pub(crate) fn handle_preset_name_key(app: &mut App, key: event::KeyEvent) {
    let ctrl = key.modifiers.contains(event::KeyModifiers::CONTROL);
    let is_ctrl_c = key.code == KeyCode::Char('c') && ctrl;
    let confirming = app
        .preset_name
        .as_ref()
        .is_some_and(|d| d.confirm_overwrite);

    if confirming {
        match key.code {
            KeyCode::Enter | KeyCode::Char('y') => {
                let _ = app.submit_preset_name(true);
            }
            KeyCode::Esc | KeyCode::Char('n') => {
                if let Some(d) = app.preset_name.as_mut() {
                    d.confirm_overwrite = false;
                }
            }
            KeyCode::Char('c') if ctrl => {
                if let Some(d) = app.preset_name.as_mut() {
                    d.confirm_overwrite = false;
                }
            }
            _ => {}
        }
        return;
    }

    if key.code == KeyCode::Esc || is_ctrl_c {
        app.preset_name = None;
        return;
    }
    if key.code == KeyCode::Enter {
        let _ = app.submit_preset_name(false);
        return;
    }
    if let Some(dialog) = app.preset_name.as_mut() {
        let _ = apply_text_field_key(&mut dialog.field, key.code, ctrl);
    }
}

/// Bookmark compare tray keys. Esc / Ctrl+C close without resuming follow.
/// Pending `d`/`y` stay on the panel; Esc with pending cancels the pending only.
pub(crate) fn handle_compare_key(app: &mut App, key: event::KeyEvent) {
    let ctrl = key.modifiers.contains(event::KeyModifiers::CONTROL);
    if key.code == KeyCode::Char('c') && ctrl {
        app.close_compare_panel();
        return;
    }
    let pending_d = app.compare.as_ref().is_some_and(|p| p.pending_d);
    let pending_y = app.compare.as_ref().is_some_and(|p| p.pending_y);
    if key.code == KeyCode::Esc {
        if pending_d || pending_y {
            if let Some(panel) = app.compare.as_mut() {
                panel.clear_pending();
            }
        } else {
            app.close_compare_panel();
        }
        return;
    }
    if pending_y {
        if matches!(key.code, KeyCode::Char('y')) {
            app.compare_yank_selected();
        } else if let Some(panel) = app.compare.as_mut() {
            panel.clear_pending();
        }
        return;
    }
    if pending_d {
        if matches!(key.code, KeyCode::Char('d')) {
            app.compare_delete_selected();
        } else if let Some(panel) = app.compare.as_mut() {
            panel.clear_pending();
        }
        return;
    }
    match key.code {
        KeyCode::Char('j') | KeyCode::Down => app.compare_move(1),
        KeyCode::Char('k') | KeyCode::Up => app.compare_move(-1),
        KeyCode::Char('g') => app.compare_goto_first(),
        KeyCode::Char('G') => app.compare_goto_last(),
        KeyCode::Char('y') => {
            if let Some(panel) = app.compare.as_mut() {
                panel.pending_y = true;
            }
        }
        KeyCode::Char('d') => {
            if let Some(panel) = app.compare.as_mut() {
                panel.pending_d = true;
            }
        }
        KeyCode::Enter => app.compare_jump_selected(),
        _ => {}
    }
}

/// Read-only Help panel keys. Esc / `?` / Ctrl+C close without resuming follow
/// except Esc while searching, which clears search first.
pub(crate) fn handle_help_key(app: &mut App, key: event::KeyEvent) {
    use keymap::ActionId;
    let ctrl = key.modifiers.contains(event::KeyModifiers::CONTROL);
    if key.code == KeyCode::Char('c') && ctrl {
        app.close_help();
        return;
    }
    if km_event(app, ActionId::HelpToggle, key) {
        app.close_help();
        return;
    }

    if app.help_search_prompting() {
        if km_event(app, ActionId::HelpClose, key) || key.code == KeyCode::Esc {
            app.help_clear_search();
            return;
        }
        if km_event(app, ActionId::HelpSubmit, key) || key.code == KeyCode::Enter {
            app.help_commit_search();
            return;
        }
        if key.code == KeyCode::Up {
            app.help_search_step_prompt(-1);
            return;
        }
        if key.code == KeyCode::Down {
            app.help_search_step_prompt(1);
            return;
        }
        let edited = if let Some(search) = app.help_search.as_mut() {
            apply_text_field_key(&mut search.query, key.code, ctrl)
        } else {
            false
        };
        if edited {
            app.help_on_query_edit();
        }
        return;
    }

    if km_event(app, ActionId::HelpClose, key) {
        if app.help_search.as_ref().is_some_and(|s| s.has_hits()) {
            app.help_clear_search();
        } else {
            app.close_help();
        }
        return;
    }

    if let KeyCode::Char(c) = key.code {
        if !ctrl {
            if let Some(page) = crate::help::HelpPage::from_digit(c) {
                app.help_open_page(page);
                return;
            }
        }
    }

    if km_event(app, ActionId::HelpSearch, key) {
        app.help_begin_search();
        return;
    }
    if km_event(app, ActionId::HelpSearchNext, key) {
        app.help_search_step(1);
        return;
    }
    if km_event(app, ActionId::HelpSearchPrev, key) {
        app.help_search_step(-1);
        return;
    }
    if km_event(app, ActionId::HelpBack, key) || km_event(app, ActionId::HelpBackAlt, key) {
        app.help_pop_to_home();
        return;
    }
    if km_event(app, ActionId::HelpSubmit, key) {
        if let crate::help::HelpView::Home { toc, .. } = app.help_view {
            if let Some(page) = crate::help::HelpPage::from_index(toc) {
                app.help_open_page(page);
            }
        }
        return;
    }
    if km_event(app, ActionId::HelpScrollDown, key) || key.code == KeyCode::Down {
        app.scroll_help(1);
    } else if km_event(app, ActionId::HelpScrollUp, key) || key.code == KeyCode::Up {
        app.scroll_help(-1);
    } else if km_event(app, ActionId::HelpJumpDown, key) {
        app.scroll_help(FAST_SCROLL_STEP);
    } else if km_event(app, ActionId::HelpJumpUp, key) {
        app.scroll_help(-FAST_SCROLL_STEP);
    } else if km_event(app, ActionId::HelpBottom, key) {
        app.help_scroll_bottom();
    } else if km_event(app, ActionId::HelpTop, key) {
        app.help_scroll_top();
    }
}

/// Read-only summary panel keys (Leader `i`). `j`/`k`/`Down`/`Up` scroll the
/// body; `Esc`/Ctrl+C close without resuming follow (same convention as
/// Detail/Help). Re-pressing the `Leader i` chord while open also closes it
/// (toggle semantics) — this is the only place `pending_leader` is armed
/// while the panel owns key routing.
pub(crate) fn handle_summary_key(app: &mut App, key: event::KeyEvent) {
    use keymap::ActionId;
    let ctrl = key.modifiers.contains(event::KeyModifiers::CONTROL);
    if key.code == KeyCode::Char('c') && ctrl {
        app.close_summary_panel();
        return;
    }
    if app.pending_leader {
        app.pending_leader = false;
        if km_code(app, ActionId::LeaderSummary, key.code) {
            app.close_summary_panel();
        }
        return;
    }
    if key.code == KeyCode::Esc {
        app.close_summary_panel();
        return;
    }
    if km_code(app, ActionId::LogListLeader, key.code) {
        app.pending_leader = true;
        return;
    }
    match key.code {
        KeyCode::Char('j') | KeyCode::Down => app.scroll_summary(1),
        KeyCode::Char('k') | KeyCode::Up => app.scroll_summary(-1),
        KeyCode::Char('J') => app.scroll_summary(FAST_SCROLL_STEP),
        KeyCode::Char('K') => app.scroll_summary(-FAST_SCROLL_STEP),
        _ => {}
    }
}

pub(crate) fn handle_hist_panel_key(app: &mut App, key: event::KeyEvent) {
    use keymap::ActionId;
    let ctrl = key.modifiers.contains(event::KeyModifiers::CONTROL);
    if key.code == KeyCode::Char('c') && ctrl {
        app.close_hist_panel();
        return;
    }
    if km_code(app, ActionId::HistPanelCancel, key.code) || key.code == KeyCode::Esc {
        app.close_hist_panel();
        return;
    }
    if km_code(app, ActionId::HistPanelPrev, key.code) || key.code == KeyCode::Up {
        app.move_hist_cursor(-1);
        return;
    }
    if km_code(app, ActionId::HistPanelNext, key.code) || key.code == KeyCode::Down {
        app.move_hist_cursor(1);
        return;
    }
    if km_code(app, ActionId::HistPanelJumpDown, key.code) {
        app.move_hist_cursor(FAST_SCROLL_STEP);
        return;
    }
    if km_code(app, ActionId::HistPanelJumpUp, key.code) {
        app.move_hist_cursor(-FAST_SCROLL_STEP);
        return;
    }
    if km_code(app, ActionId::HistPanelJumpTop, key.code) {
        app.jump_hist_cursor(false);
        return;
    }
    if km_code(app, ActionId::HistPanelJumpBottom, key.code) {
        app.jump_hist_cursor(true);
        return;
    }
    if km_event(app, ActionId::HistPanelZoomOut, key) || key.code == KeyCode::Tab {
        app.zoom_hist(false);
        return;
    }
    if km_event(app, ActionId::HistPanelZoomIn, key) || key.code == KeyCode::BackTab {
        app.zoom_hist(true);
        return;
    }
    if km_code(app, ActionId::HistPanelSubmit, key.code) || key.code == KeyCode::Enter {
        app.submit_hist_jump();
        return;
    }
    if km_code(app, ActionId::HistPanelApplyWindow, key.code) {
        app.apply_hist_window();
    }
}

/// Route keys to the open `tt` time panel. Esc / Ctrl+C cancel without applying
/// and do not resume following (same draft-cancel convention as Picker).
pub(crate) fn handle_time_panel_key(app: &mut App, key: event::KeyEvent) {
    use time_panel::TimePanelOutcome;

    let ctrl = key.modifiers.contains(event::KeyModifiers::CONTROL);
    if key.code == KeyCode::Esc || (key.code == KeyCode::Char('c') && ctrl) {
        app.close_time_panel();
        return;
    }

    let Some(panel) = app.time_panel.as_mut() else {
        return;
    };
    match panel.handle_key(key.code) {
        TimePanelOutcome::Continue => {}
        TimePanelOutcome::Cancel => {
            app.close_time_panel();
        }
        TimePanelOutcome::Submit(bound) => {
            app.apply_time_bound(bound);
        }
        TimePanelOutcome::Flash(msg) => {
            app.set_flash(msg);
        }
    }
}

pub(crate) fn handle_palette_key(app: &mut App, key: event::KeyEvent) {
    use keymap::ActionId;
    let ctrl = key.modifiers.contains(event::KeyModifiers::CONTROL);
    let is_ctrl_c = key.code == KeyCode::Char('c') && ctrl;
    if is_ctrl_c || km_event(app, ActionId::PaletteClose, key) {
        app.close_command_palette();
        return;
    }
    if km_event(app, ActionId::PaletteSubmit, key) {
        let query = app
            .command_palette
            .as_ref()
            .map(|p| p.query.as_str().to_string())
            .unwrap_or_default();
        let hits = crate::action::filtered_catalog(app, &query);
        if hits.is_empty() {
            return;
        }
        let selected = app
            .command_palette
            .as_ref()
            .map(|p| p.selected.min(hits.len() - 1))
            .unwrap_or(0);
        let id = hits[selected].id;
        app.close_command_palette();
        crate::action::dispatch(app, id);
        return;
    }
    if km_event(app, ActionId::PaletteDown, key) {
        let query = app
            .command_palette
            .as_ref()
            .map(|p| p.query.as_str().to_string())
            .unwrap_or_default();
        let n = crate::action::filtered_catalog(app, &query).len();
        if let Some(p) = app.command_palette.as_mut() {
            p.move_sel(1, n);
        }
        return;
    }
    if km_event(app, ActionId::PaletteUp, key) {
        let query = app
            .command_palette
            .as_ref()
            .map(|p| p.query.as_str().to_string())
            .unwrap_or_default();
        let n = crate::action::filtered_catalog(app, &query).len();
        if let Some(p) = app.command_palette.as_mut() {
            p.move_sel(-1, n);
        }
        return;
    }
    if let Some(p) = app.command_palette.as_mut() {
        if !apply_text_field_key(&mut p.query, key.code, ctrl) {
            return;
        }
    }
    let query = app
        .command_palette
        .as_ref()
        .map(|p| p.query.as_str().to_string())
        .unwrap_or_default();
    let n = crate::action::filtered_catalog(app, &query).len();
    if let Some(p) = app.command_palette.as_mut() {
        p.clamp_selected(n);
    }
}
