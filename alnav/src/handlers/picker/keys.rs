//! Picker key dispatch (`handle_picker_key`).

use crate::app::App;
use crate::handlers::apply_yank;
use crate::handlers::picker::level_field_candidates;
use crate::handlers::picker::render_data::{
    highlight_create_row_selected, highlight_manage_len, highlight_selected_index,
    preset_visible_indices, unified_selected_id, unified_visible_ids,
};
use crate::handlers::picker::submit::{
    apply_text_field_key, handle_confirm_key, manage_enter_edit_selected,
    manage_request_delete_selected, manage_toggle_all, maybe_highlight_auto_new,
    maybe_return_highlight_auto_new, replace_last_token, submit_filter_picker,
    submit_highlight_create_query, submit_highlight_picker,
};
use crossterm::event::{self, KeyCode};

pub(crate) fn handle_picker_key(app: &mut App, key: event::KeyEvent) {
    use crate::picker::{PickerKind, PickerMode, PickerSession};

    let ctrl = key.modifiers.contains(event::KeyModifiers::CONTROL);
    if handle_confirm_key(app, key) {
        return;
    }

    let is_ctrl_c = key.code == KeyCode::Char('c') && ctrl;
    if key.code == KeyCode::Esc || is_ctrl_c {
        // Any mode: close the picker (confirm dialog handled above).
        app.close_picker();
        return;
    }

    let Some((kind, mode)) = app
        .picker
        .as_ref()
        .map(|session| (session.kind.clone(), session.mode.clone()))
    else {
        return;
    };

    if matches!(mode, PickerMode::Manage) {
        match kind {
            PickerKind::ActionList { .. } => match key.code {
                KeyCode::Enter | KeyCode::Tab => {
                    let _ = app.confirm_msg_action_list();
                }
                KeyCode::Up => {
                    let selected = app.picker.as_ref().unwrap().selected.saturating_sub(1);
                    app.picker.as_mut().unwrap().selected = selected;
                }
                KeyCode::Down => {
                    let count = {
                        let session = app.picker.as_ref().unwrap();
                        PickerSession::contains_indices(&session.choices, session.query.as_str())
                            .len()
                    };
                    let session = app.picker.as_mut().unwrap();
                    session.selected = (session.selected + 1).min(count.saturating_sub(1));
                }
                code => {
                    let session = app.picker.as_mut().unwrap();
                    if apply_text_field_key(&mut session.query, code, ctrl) {
                        session.selected = 0;
                    }
                }
            },
            PickerKind::Preset => {
                let vis = preset_visible_indices(app);
                let is_delete = matches!(key.code, KeyCode::Delete)
                    || (matches!(key.code, KeyCode::Char('d')) && ctrl);
                match key.code {
                    KeyCode::Up => {
                        app.picker.as_mut().unwrap().selected =
                            app.picker.as_ref().unwrap().selected.saturating_sub(1);
                    }
                    KeyCode::Down => {
                        let session = app.picker.as_mut().unwrap();
                        session.selected = (session.selected + 1).min(vis.len().saturating_sub(1));
                    }
                    KeyCode::Tab => {}
                    KeyCode::Char('x') if ctrl => {
                        if let Some(&idx) = vis.get(app.picker.as_ref().unwrap().selected) {
                            let name = app.preset_catalog[idx].name.clone();
                            app.preset_name = Some(crate::preset::PresetNameDialog::rename(&name));
                        }
                    }
                    _ if is_delete => {
                        if let Some(&idx) = vis.get(app.picker.as_ref().unwrap().selected) {
                            let name = app.preset_catalog[idx].name.clone();
                            app.request_confirm(crate::picker::ConfirmKind::DeletePreset { name });
                        }
                    }
                    KeyCode::Enter => {
                        if let Some(&idx) = vis.get(app.picker.as_ref().unwrap().selected) {
                            let preset = app.preset_catalog[idx].clone();
                            match app.apply_preset(&preset) {
                                Ok(()) => {
                                    app.close_picker();
                                    app.set_flash("PRESET APPLIED");
                                }
                                Err(e) => app.set_flash(&e),
                            }
                        }
                    }
                    code => {
                        let session = app.picker.as_mut().unwrap();
                        if apply_text_field_key(&mut session.query, code, ctrl) {
                            session.selected = 0;
                        }
                    }
                }
                return;
            }
            PickerKind::Highlight => {
                let list_len = highlight_manage_len(app);
                let is_delete = matches!(key.code, KeyCode::Delete)
                    || (matches!(key.code, KeyCode::Char('d')) && ctrl);
                match key.code {
                    KeyCode::Up => {
                        app.picker.as_mut().unwrap().selected =
                            app.picker.as_ref().unwrap().selected.saturating_sub(1);
                    }
                    KeyCode::Down => {
                        let session = app.picker.as_mut().unwrap();
                        session.selected = (session.selected + 1).min(list_len.saturating_sub(1));
                    }
                    KeyCode::Tab => {}
                    KeyCode::Char('x') if ctrl => {
                        if let Some(idx) = highlight_selected_index(app) {
                            let pattern = app.highlight_groups.groups[idx].pattern.clone();
                            app.picker.as_mut().unwrap().enter_edit(idx, pattern);
                        }
                    }
                    _ if is_delete => {
                        if let Some(idx) = highlight_selected_index(app) {
                            app.request_delete_many(vec![crate::picker::UnifiedId {
                                kind: crate::picker::UnifiedKind::Highlight,
                                source_index: idx,
                            }]);
                        }
                    }
                    KeyCode::Enter => {
                        if highlight_create_row_selected(app) {
                            submit_highlight_create_query(app);
                        } else if let Some(idx) = highlight_selected_index(app) {
                            let _ = app.activate_highlight_group(idx);
                            app.close_picker();
                        }
                    }
                    code => {
                        let session = app.picker.as_mut().unwrap();
                        if apply_text_field_key(&mut session.query, code, ctrl) {
                            session.selected = 0;
                            maybe_highlight_auto_new(app);
                        }
                    }
                }
                return;
            }
            _ => {
                // Unified panel (Filter/Highlight/Exclude).
                let ids = unified_visible_ids(app);
                let is_delete = matches!(key.code, KeyCode::Delete)
                    || (matches!(key.code, KeyCode::Char('d')) && ctrl);
                match key.code {
                    KeyCode::Up => {
                        app.picker.as_mut().unwrap().selected =
                            app.picker.as_ref().unwrap().selected.saturating_sub(1);
                    }
                    KeyCode::Down => {
                        let session = app.picker.as_mut().unwrap();
                        session.selected = (session.selected + 1).min(ids.len().saturating_sub(1));
                    }
                    KeyCode::Tab => {
                        let Some(id) = unified_selected_id(app) else {
                            return;
                        };
                        let session = app.picker.as_mut().unwrap();
                        if session.checked.contains(&id) {
                            session.checked.remove(&id);
                        } else {
                            session.checked.insert(id);
                        }
                        let count = ids.len();
                        session.selected = (session.selected + 1).min(count.saturating_sub(1));
                    }
                    KeyCode::Char('t') if ctrl => manage_toggle_all(app),
                    KeyCode::Char('k') if ctrl => app.request_clear_all_rules(),
                    KeyCode::Char('x') if ctrl => manage_enter_edit_selected(app),
                    _ if is_delete => manage_request_delete_selected(app),
                    KeyCode::Enter => {
                        let session = app.picker.as_ref().unwrap();
                        if !session.checked.is_empty() {
                            let items: Vec<_> = session.checked.iter().copied().collect();
                            for id in items {
                                app.toggle_unified_enabled(id.kind, id.source_index);
                            }
                            return;
                        }
                        let Some(id) = unified_selected_id(app) else {
                            return;
                        };
                        app.toggle_unified_enabled(id.kind, id.source_index);
                    }
                    code => {
                        let session = app.picker.as_mut().unwrap();
                        if apply_text_field_key(&mut session.query, code, ctrl) {
                            session.selected = 0;
                        }
                    }
                }
            }
        }
        return;
    }

    // New / Edit modes
    match kind {
        PickerKind::Highlight => match key.code {
            KeyCode::Enter => submit_highlight_picker(app),
            KeyCode::Up => {
                let selected = app.picker.as_ref().unwrap().selected.saturating_sub(1);
                app.picker.as_mut().unwrap().selected = selected;
            }
            KeyCode::Down => {
                let (draft, sel) = {
                    let s = app.picker.as_ref().unwrap();
                    (s.draft.to_string(), s.selected)
                };
                if !draft.is_empty() {
                    let n = app.vocab_match.display_labels().len();
                    app.picker.as_mut().unwrap().selected = (sel + 1).min(n.saturating_sub(1));
                }
            }
            KeyCode::Tab => {
                let (draft, selected) = {
                    let session = app.picker.as_ref().unwrap();
                    (session.draft.to_string(), session.selected)
                };
                if !draft.is_empty() {
                    let replacement = app.vocab_match.display_labels().get(selected).cloned();
                    if let Some(replacement) = replacement {
                        let new_draft = replace_last_token(&draft, &replacement);
                        let session = app.picker.as_mut().unwrap();
                        session.draft = crate::text_field::TextField::from_text(new_draft);
                        session.selected = 0;
                    }
                }
            }
            KeyCode::Backspace if ctrl => {
                let session = app.picker.as_mut().unwrap();
                session.draft.kill_word_back();
                session.selected = 0;
                maybe_return_highlight_auto_new(app);
            }
            KeyCode::Delete => {}
            code => {
                let session = app.picker.as_mut().unwrap();
                if apply_text_field_key(&mut session.draft, code, ctrl) {
                    session.selected = 0;
                    maybe_return_highlight_auto_new(app);
                }
            }
        },
        PickerKind::Filter | PickerKind::Exclude => match key.code {
            KeyCode::Enter => submit_filter_picker(app),
            KeyCode::Tab => {
                let has_field = app
                    .picker
                    .as_ref()
                    .unwrap()
                    .input
                    .as_ref()
                    .map(|i| i.draft_field.is_some())
                    .unwrap_or(false);

                if has_field {
                    let selected = app.picker.as_ref().unwrap().selected;
                    let labels = {
                        let session = app.picker.as_ref().unwrap();
                        let input = session.input.as_ref().unwrap();
                        use crate::input::ChipField;
                        match input.draft_field.unwrap() {
                            ChipField::Tag | ChipField::Pkg | ChipField::Msg => {
                                app.vocab_match.display_labels().to_vec()
                            }
                            ChipField::Level => level_field_candidates(input.draft.as_str()),
                            ChipField::Pid | ChipField::Tid => vec![],
                        }
                    };
                    if let Some(value) = labels.into_iter().nth(selected) {
                        if let Some(input) = app.picker.as_mut().unwrap().input.as_mut() {
                            input.draft = crate::text_field::TextField::from_text(value);
                        }
                        app.picker.as_mut().unwrap().selected = 0;
                    }
                } else {
                    let confirmed = app
                        .picker
                        .as_mut()
                        .unwrap()
                        .input
                        .as_mut()
                        .map(|i| i.confirm_field_candidate())
                        .unwrap_or(false);
                    if confirmed {
                        app.picker.as_mut().unwrap().selected = 0;
                    }
                }
            }
            KeyCode::Up => {
                let has_field = app
                    .picker
                    .as_ref()
                    .unwrap()
                    .input
                    .as_ref()
                    .map(|i| i.draft_field.is_some())
                    .unwrap_or(false);
                if has_field {
                    let sel = app.picker.as_ref().unwrap().selected;
                    app.picker.as_mut().unwrap().selected = sel.saturating_sub(1);
                } else if let Some(input) = app.picker.as_mut().unwrap().input.as_mut() {
                    input.move_field_selection(-1);
                }
            }
            KeyCode::Down => {
                let has_field = app
                    .picker
                    .as_ref()
                    .unwrap()
                    .input
                    .as_ref()
                    .map(|i| i.draft_field.is_some())
                    .unwrap_or(false);
                if has_field {
                    let labels_len = {
                        let session = app.picker.as_ref().unwrap();
                        let input = session.input.as_ref().unwrap();
                        use crate::input::ChipField;
                        match input.draft_field.unwrap() {
                            ChipField::Tag | ChipField::Pkg | ChipField::Msg => {
                                app.vocab_match.display_labels().len()
                            }
                            ChipField::Level => level_field_candidates(input.draft.as_str()).len(),
                            ChipField::Pid | ChipField::Tid => 0,
                        }
                    };
                    let sel = app.picker.as_ref().unwrap().selected;
                    app.picker.as_mut().unwrap().selected =
                        (sel + 1).min(labels_len.saturating_sub(1));
                } else if let Some(input) = app.picker.as_mut().unwrap().input.as_mut() {
                    input.move_field_selection(1);
                }
            }
            KeyCode::Backspace if ctrl => {
                if let Some(input) = app.picker.as_mut().unwrap().input.as_mut() {
                    input.draft.kill_word_back();
                    input.field_selected = 0;
                }
            }
            KeyCode::Delete => {}
            KeyCode::Backspace => {
                if let Some(input) = app.picker.as_mut().unwrap().input.as_mut() {
                    input.backspace();
                }
            }
            KeyCode::Char(c) if !ctrl => {
                let exclude_has_chip = kind == PickerKind::Exclude
                    && app
                        .picker
                        .as_ref()
                        .and_then(|session| session.input.as_ref())
                        .is_some_and(|input| !input.chips.is_empty());
                if exclude_has_chip {
                    app.set_flash("EXCLUDE NEEDS ONE CHIP");
                } else if let Some(input) = app.picker.as_mut().unwrap().input.as_mut() {
                    input.push_char(c);
                }
            }
            code => {
                if let Some(input) = app.picker.as_mut().unwrap().input.as_mut() {
                    let _ = apply_text_field_key(&mut input.draft, code, ctrl);
                }
            }
        },
        PickerKind::MsgChip { .. } => match key.code {
            KeyCode::Enter | KeyCode::Tab => {
                if let Some(text) = app.confirm_msg_token_picker() {
                    apply_yank(app, text);
                }
            }
            KeyCode::Up => {
                let selected = app.picker.as_ref().unwrap().selected.saturating_sub(1);
                app.picker.as_mut().unwrap().selected = selected;
            }
            KeyCode::Down => {
                let session = app.picker.as_ref().unwrap();
                let count =
                    PickerSession::filtered_indices(&session.choices, session.draft.as_str()).len();
                app.picker.as_mut().unwrap().selected =
                    (session.selected + 1).min(count.saturating_sub(1));
            }
            KeyCode::Backspace if ctrl => {
                let session = app.picker.as_mut().unwrap();
                session.draft.kill_word_back();
                session.selected = 0;
            }
            KeyCode::Delete => {}
            code => {
                let session = app.picker.as_mut().unwrap();
                if apply_text_field_key(&mut session.draft, code, ctrl) {
                    session.selected = 0;
                }
            }
        },
        PickerKind::Unified | PickerKind::ActionList { .. } => {}
        PickerKind::Preset => {}
    }
}
