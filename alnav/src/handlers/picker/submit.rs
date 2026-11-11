//! Picker submit / confirm / manage actions (highlight & filter submit, delete, toggle-all, edit).

use crate::app::App;
use crate::handlers::picker::render_data::{
    highlight_create_query, highlight_visible_indices, preset_visible_indices,
    unified_picker_items, unified_selected_id, unified_visible_ids,
};
use crossterm::event::{self, KeyCode};

pub(super) fn submit_highlight_create_query(app: &mut App) {
    let Some(query) = highlight_create_query(app).map(str::to_string) else {
        return;
    };
    let Some(group) = crate::highlight_model::HighlightGroup::from_pattern(&query) else {
        return;
    };
    let idx = app.push_or_find_highlight_group(group);
    let _ = app.activate_highlight_group(idx);
    app.close_picker();
}

pub(super) fn maybe_highlight_auto_new(app: &mut App) {
    use crate::picker::{PickerKind, PickerMode};
    let Some(session) = app.picker.as_ref() else {
        return;
    };
    if session.kind != PickerKind::Highlight || !matches!(session.mode, PickerMode::Manage) {
        return;
    }
    if session.query.is_empty() {
        return;
    }
    if !highlight_visible_indices(app).is_empty() {
        return;
    }
    let draft = app.picker.as_ref().unwrap().query.to_string();
    app.picker.as_mut().unwrap().enter_new_with_draft(draft);
}

pub(super) fn maybe_return_highlight_auto_new(app: &mut App) {
    use crate::picker::{PickerKind, PickerMode};
    let Some(session) = app.picker.as_ref() else {
        return;
    };
    if session.kind != PickerKind::Highlight
        || !matches!(session.mode, PickerMode::New)
        || !session.auto_from_manage
        || !session.draft.is_empty()
        || app.highlight_groups.groups.is_empty()
    {
        return;
    }
    app.picker.as_mut().unwrap().return_to_manage();
}

fn confirm_pending(app: &mut App) {
    use crate::picker::ConfirmKind;

    let Some(confirm) = app.confirm.clone() else {
        return;
    };
    match confirm {
        ConfirmKind::DeleteMany { mut items } => {
            // Delete high indices first within each kind so earlier indices stay valid.
            items.sort_by(|a, b| {
                a.kind
                    .tag()
                    .cmp(b.kind.tag())
                    .then(b.source_index.cmp(&a.source_index))
            });
            for id in &items {
                app.delete_unified_at(id.kind, id.source_index);
            }
            let highlight_manage = app
                .picker
                .as_ref()
                .is_some_and(|s| matches!(s.kind, crate::picker::PickerKind::Highlight));
            if highlight_manage {
                // Empty groups must not stay in Manage (`/` with none → New).
                if app.highlight_groups.groups.is_empty() {
                    app.cancel_confirm();
                    if let Some(session) = app.picker.as_mut() {
                        session.enter_new();
                    }
                    return;
                }
                let count = highlight_visible_indices(app).len();
                app.cancel_confirm();
                if let Some(session) = app.picker.as_mut() {
                    session.checked.clear();
                    session.selected = session.selected.min(count.saturating_sub(1));
                }
                maybe_highlight_auto_new(app);
                return;
            }
            let count = unified_picker_items(app).len();
            app.cancel_confirm();
            if let Some(session) = app.picker.as_mut() {
                session.checked.clear();
                session.selected = session.selected.min(count.saturating_sub(1));
            }
        }
        ConfirmKind::DeletePreset { name } => {
            app.delete_preset_named(&name);
            let count = preset_visible_indices(app).len();
            if count == 0 {
                app.close_picker();
            } else {
                app.cancel_confirm();
                if let Some(session) = app.picker.as_mut() {
                    session.selected = session.selected.min(count.saturating_sub(1));
                }
            }
        }
        ConfirmKind::ClearAll => app.clear_all_rules(),
    }
}

/// Swallow keys while a screen-centered confirm is open. Returns true when handled.
pub(crate) fn handle_confirm_key(app: &mut App, key: event::KeyEvent) -> bool {
    if app.confirm.is_none() {
        return false;
    }
    match key.code {
        KeyCode::Enter | KeyCode::Char('y') => confirm_pending(app),
        KeyCode::Esc | KeyCode::Char('n') => app.cancel_confirm(),
        _ => {}
    }
    true
}

pub(crate) fn replace_last_token(draft: &str, replacement: &str) -> String {
    match draft.rfind(' ') {
        Some(pos) => format!("{} {}", &draft[..pos], replacement),
        None => replacement.to_string(),
    }
}

pub(super) fn submit_highlight_picker(app: &mut App) {
    use crate::picker::PickerMode;

    let Some((mode, draft)) = app
        .picker
        .as_ref()
        .map(|session| (session.mode.clone(), session.draft.clone()))
    else {
        return;
    };
    let mut highlight_box = crate::highlight_model::HighlightBox {
        draft,
        editing: true,
        selected: 0,
    };
    let Ok(Some(group)) = highlight_box.submit_draft() else {
        return;
    };
    let idx = match mode {
        PickerMode::New => app.push_or_find_highlight_group(group),
        PickerMode::Edit { index } => {
            if !app.update_search_group(index, &group.pattern) {
                app.set_flash("EXISTS");
                return;
            }
            index
        }
        PickerMode::Manage => return,
    };
    // Exact reuse of a disabled group must enable before jump_first_match_of.
    let _ = app.activate_highlight_group(idx);
    app.close_picker();
}

pub(super) fn submit_filter_picker(app: &mut App) {
    use crate::picker::{PickerKind, PickerMode};

    let Some((kind, mode)) = app
        .picker
        .as_ref()
        .map(|session| (session.kind.clone(), session.mode.clone()))
    else {
        return;
    };

    {
        let Some(input) = app
            .picker
            .as_mut()
            .and_then(|session| session.input.as_mut())
        else {
            return;
        };
        if input.confirm_field_candidate_on_enter() {
            return;
        }
        if input.has_pending_draft() {
            input.commit_draft_as_chip();
            return;
        }
        if input.is_empty() {
            return;
        }
    }
    if kind == PickerKind::Exclude
        && app
            .picker
            .as_ref()
            .and_then(|session| session.input.as_ref())
            .is_some_and(|input| input.chips.len() != 1)
    {
        app.set_flash("EXCLUDE NEEDS ONE CHIP");
        return;
    }

    let selected = match (&kind, mode) {
        (PickerKind::Filter, PickerMode::New) => {
            let group = {
                let input = app.picker.as_mut().unwrap().input.as_mut().unwrap();
                match input.build_group(true) {
                    Ok(Some(group)) => group,
                    Ok(None) => return,
                    Err(error) => {
                        app.set_flash(error);
                        return;
                    }
                }
            };
            if let Some(index) = app.groups.groups.iter().position(|g| g.same_as(&group)) {
                index
            } else {
                app.push_filter_group(group);
                app.rebuild_visible();
                app.groups.groups.len() - 1
            }
        }
        (PickerKind::Filter, PickerMode::Edit { index }) => {
            let group = {
                let input = app.picker.as_mut().unwrap().input.as_mut().unwrap();
                match input.build_group(true) {
                    Ok(Some(group)) => group,
                    Ok(None) => return,
                    Err(error) => {
                        app.set_flash(error);
                        return;
                    }
                }
            };
            if !app.update_filter_group(index, group) {
                app.set_flash("EXISTS");
                return;
            }
            index
        }
        (PickerKind::Exclude, PickerMode::New) => {
            let chips = {
                let input = app.picker.as_mut().unwrap().input.as_mut().unwrap();
                std::mem::take(&mut input.chips)
            };
            let before = app.groups.excludes.len();
            for chip in chips {
                app.push_exclude_chip(chip);
            }
            if app.groups.excludes.is_empty() {
                return;
            }
            if app.groups.excludes.len() == before {
                before.saturating_sub(1)
            } else {
                app.groups.excludes.len() - 1
            }
        }
        (PickerKind::Exclude, PickerMode::Edit { index }) => {
            let group = {
                let input = app.picker.as_mut().unwrap().input.as_mut().unwrap();
                match input.build_group(true) {
                    Ok(Some(group)) => group,
                    Ok(None) => return,
                    Err(error) => {
                        app.set_flash(error);
                        return;
                    }
                }
            };
            if !app.update_exclude_group(index, group) {
                app.set_flash("EXISTS");
                return;
            }
            index
        }
        _ => return,
    };
    match &kind {
        PickerKind::Filter => app.group_cursor = selected,
        PickerKind::Exclude => app.exclude_cursor = selected,
        _ => {}
    }
    app.close_picker();
}

/// Shared readline-subset keys for a [`text_field::TextField`].
/// Does not handle Ctrl-Backspace or Delete (mode-specific).
pub(crate) fn apply_text_field_key(
    field: &mut crate::text_field::TextField,
    code: KeyCode,
    ctrl: bool,
) -> bool {
    crate::text_field::apply_key(field, code, ctrl)
}

pub(super) fn manage_request_delete_selected(app: &mut App) {
    let session = app.picker.as_ref().unwrap();
    let items: Vec<crate::picker::UnifiedId> = if session.checked.is_empty() {
        unified_selected_id(app).into_iter().collect()
    } else {
        session.checked.iter().copied().collect()
    };
    if !items.is_empty() {
        app.request_delete_many(items);
    }
}

/// `C-t` in unified Manage: flip every target item on or off in one shot.
/// Targets are the Tab-checked set when non-empty, else the query-visible rows.
/// Any enabled target → disable all; none enabled → enable all.
pub(super) fn manage_toggle_all(app: &mut App) {
    let checked: Vec<crate::picker::UnifiedId> = app
        .picker
        .as_ref()
        .map(|session| session.checked.iter().copied().collect())
        .unwrap_or_default();
    let targets = if checked.is_empty() {
        unified_visible_ids(app)
    } else {
        checked
    };
    if targets.is_empty() {
        return;
    }
    let any_enabled = targets
        .iter()
        .any(|&id| app.unified_enabled(id).unwrap_or(false));
    let enable = !any_enabled;
    app.set_unified_enabled_bulk(&targets, enable);
    app.set_flash(if enable {
        "ALL ENABLED"
    } else {
        "ALL DISABLED"
    });
}

pub(super) fn manage_enter_edit_selected(app: &mut App) {
    if app.picker.as_ref().is_some_and(|s| s.checked.len() > 1) {
        app.set_flash("NO BULK EDIT");
        return;
    }
    let Some(id) = (|| {
        let session = app.picker.as_ref()?;
        if session.checked.len() == 1 {
            session.checked.iter().next().copied()
        } else {
            unified_selected_id(app)
        }
    })() else {
        return;
    };
    use crate::picker::UnifiedKind;
    match id.kind {
        UnifiedKind::Highlight => {
            let pattern = app.highlight_groups.groups[id.source_index].pattern.clone();
            let session = app.picker.as_mut().unwrap();
            session.kind = id.kind.as_picker_kind();
            session.enter_edit(id.source_index, pattern);
        }
        UnifiedKind::Filter => {
            let input = crate::input::InputBox {
                chips: app.groups.groups[id.source_index].chips.clone(),
                ..crate::input::InputBox::default()
            };
            let session = app.picker.as_mut().unwrap();
            session.kind = id.kind.as_picker_kind();
            session.enter_edit_input(id.source_index, input);
        }
        UnifiedKind::Exclude => {
            let input = crate::input::InputBox {
                chips: vec![app.groups.excludes[id.source_index].chip.clone()],
                exclude_mode: true,
                ..crate::input::InputBox::default()
            };
            let session = app.picker.as_mut().unwrap();
            session.kind = id.kind.as_picker_kind();
            session.enter_edit_input(id.source_index, input);
        }
    }
}
