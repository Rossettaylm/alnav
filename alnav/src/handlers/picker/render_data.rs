//! Picker view model (`PickerRenderData`) and visible-row / selection helpers.

use crate::app::App;
use crate::handlers::picker::level_field_candidates;
use crate::{input, preview, theme, ui};

pub(crate) struct PickerRenderData {
    pub(crate) title: String,
    pub(crate) mode: crate::picker::PickerMode,
    pub(crate) text: String,
    /// Char-index caret into `text` for the search prompt.
    pub(crate) caret: usize,
    /// Query used to color substring matches in the candidate list.
    pub(crate) match_query: String,
    pub(crate) chips: Vec<input::Chip>,
    pub(crate) exclude_chips: bool,
    /// Confirmed field awaiting a value (`tag:` prefix); Filter/Exclude New·Edit only.
    pub(crate) draft_field: Option<input::ChipField>,
    pub(crate) labels: Vec<String>,
    pub(crate) styles: Vec<ratatui::style::Style>,
    pub(crate) checked: Vec<bool>,
    pub(crate) actions: Vec<crate::ui::ActionKind>,
    pub(crate) selected: usize,
    pub(crate) empty_msg: String,
    pub(crate) preview: Vec<preview::PreviewHit>,
    /// Whether the right preview pane should be rendered (layout-level toggle).
    pub(crate) show_preview: bool,
    /// Preset panel: chip-strip Preview lines (overrides hits when set).
    pub(crate) preset_preview: Option<Vec<ratatui::text::Line<'static>>>,
    /// Search-line leading nerdfont; `None` derives from [`PickerMode`].
    pub(crate) prompt_icon: Option<&'static str>,
}

pub(crate) fn picker_render_data(
    app: &App,
    preview_limit: usize,
    preview_inner_width: u16,
) -> Option<PickerRenderData> {
    use crate::picker::{PickerKind, PickerMode, PickerSession, UnifiedKind};

    let session = app.picker.as_ref()?;
    let (title, mode, text, match_query, caret) = match &session.kind {
        PickerKind::ActionList { .. } => (
            "Create".to_string(),
            session.mode.clone(),
            session.query.to_string(),
            session.query.to_string(),
            session.query.cursor(),
        ),
        _ => {
            let base_title = match &session.kind {
                PickerKind::Unified => "Manage",
                PickerKind::Filter => "Filter",
                PickerKind::Highlight => "Highlight",
                PickerKind::Exclude => "Exclude",
                PickerKind::Preset => "Preset",
                PickerKind::MsgChip { .. } => "Message",
                PickerKind::ActionList { .. } => unreachable!(),
            };
            let mode_name = match session.mode {
                PickerMode::Manage => "Search",
                PickerMode::New => "New",
                PickerMode::Edit { .. } => "Edit",
            };
            let title = format!("{base_title} · {mode_name}");
            let mode = session.mode.clone();
            let text = match session.mode {
                PickerMode::Manage => session.query.to_string(),
                PickerMode::New | PickerMode::Edit { .. } => match &session.kind {
                    PickerKind::Filter | PickerKind::Exclude => session
                        .input
                        .as_ref()
                        .map(|input| input.draft.to_string())
                        .unwrap_or_default(),
                    _ => session.draft.to_string(),
                },
            };
            let match_query = text.clone();
            let caret = match session.mode {
                PickerMode::Manage => session.query.cursor(),
                PickerMode::New | PickerMode::Edit { .. } => match &session.kind {
                    PickerKind::Filter | PickerKind::Exclude => session
                        .input
                        .as_ref()
                        .map(|input| input.draft.cursor())
                        .unwrap_or(0),
                    _ => session.draft.cursor(),
                },
            };
            (title, mode, text, match_query, caret)
        }
    };
    let mut show_preview = app.config.picker_preview_enabled
        && !matches!(
            session.kind,
            PickerKind::Unified | PickerKind::ActionList { .. }
        );
    let mut selected = session.selected;
    let mut chips = Vec::new();
    let mut exclude_chips = false;
    let mut draft_field = None;
    let mut labels = Vec::new();
    let mut styles = Vec::new();
    let mut checked = Vec::new();
    let mut actions = Vec::new();
    let mut preview_lines = Vec::new();
    let mut empty_msg = "no items".to_string();
    let mut preset_preview: Option<Vec<ratatui::text::Line<'static>>> = None;

    match session.mode {
        PickerMode::Manage => match &session.kind {
            PickerKind::ActionList { .. } => {
                // Substring filter only (not fuzzy) — two fixed choices.
                let visible =
                    PickerSession::contains_indices(&session.choices, session.query.as_str());
                labels = visible
                    .iter()
                    .map(|&i| session.choices[i].clone())
                    .collect();
                styles = vec![theme::muted(); labels.len()];
                checked = vec![false; labels.len()];
                actions = vec![crate::ui::ActionKind::None; labels.len()];
                empty_msg = "no match".to_string();
                show_preview = false;
            }
            PickerKind::Preset => {
                let vis = preset_visible_indices(app);
                labels = vis
                    .iter()
                    .map(|&i| app.preset_catalog[i].name.clone())
                    .collect();
                styles = vec![theme::muted(); labels.len()];
                checked = vec![false; labels.len()];
                actions = vec![crate::ui::ActionKind::Jump; labels.len()];
                empty_msg = "no presets".to_string();
                if show_preview {
                    if let Some(&idx) = vis.get(session.selected) {
                        preset_preview = Some(ui::preset_preview_lines(
                            &app.preset_catalog[idx],
                            preview_inner_width,
                        ));
                    }
                }
            }
            PickerKind::Highlight => {
                let vis = highlight_visible_indices(app);
                labels = vis
                    .iter()
                    .map(|&i| app.highlight_groups.groups[i].pattern.clone())
                    .collect();
                styles = vis
                    .iter()
                    .map(|&i| {
                        if app.highlight_groups.groups[i].enabled {
                            theme::unified_kind_style(crate::picker::UnifiedKind::Highlight)
                        } else {
                            theme::disabled_chip_style()
                        }
                    })
                    .collect();
                checked = vec![false; vis.len()];
                actions = vec![crate::ui::ActionKind::Jump; vis.len()];
                if let Some(query) = highlight_create_query(app) {
                    labels.push(highlight_create_row_label(query));
                    styles.push(theme::picker_mode_style());
                    checked.push(false);
                    actions.push(crate::ui::ActionKind::None);
                }
                empty_msg = "no highlights".to_string();
                if show_preview {
                    if highlight_create_row_selected(app) {
                        if let Some(query) = highlight_create_query(app) {
                            preview_lines =
                                preview::preview_highlight_pattern_lines(app, query, preview_limit)
                                    .unwrap_or_default();
                        }
                    } else if let Some(&idx) = vis.get(session.selected) {
                        preview_lines = preview::preview_highlight_pattern_lines(
                            app,
                            &app.highlight_groups.groups[idx].pattern,
                            preview_limit,
                        )
                        .unwrap_or_default();
                    }
                }
            }
            _ => {
                // Unified panel: Filter/Highlight/Exclude only (bookmark arm removed).
                let all = unified_picker_items(app);
                let all_labels: Vec<String> = all.iter().map(|item| item.label.clone()).collect();
                let visible = PickerSession::filtered_indices(&all_labels, session.query.as_str());
                labels = visible
                    .iter()
                    .map(|&index| all[index].label.clone())
                    .collect();
                styles = visible
                    .iter()
                    .map(|&index| {
                        let item = &all[index];
                        if item.enabled {
                            theme::unified_kind_style(item.id.kind)
                        } else {
                            theme::disabled_chip_style()
                        }
                    })
                    .collect();
                checked = visible
                    .iter()
                    .map(|&index| session.checked.contains(&all[index].id))
                    .collect();
                actions = visible
                    .iter()
                    .map(|&index| crate::ui::ActionKind::Toggle {
                        enabled: all[index].enabled,
                    })
                    .collect();

                if show_preview {
                    if let Some(&src) = visible.get(session.selected) {
                        let item = &all[src];
                        match item.id.kind {
                            UnifiedKind::Highlight => {
                                preview_lines = preview::preview_highlight_pattern_lines(
                                    app,
                                    &app.highlight_groups.groups[item.id.source_index].pattern,
                                    preview_limit,
                                )
                                .unwrap_or_default();
                            }
                            UnifiedKind::Filter => {
                                let input = input::InputBox {
                                    chips: app.groups.groups[item.id.source_index].chips.clone(),
                                    ..input::InputBox::default()
                                };
                                preview_lines = app.preview_filter_throttled(&input, preview_limit);
                            }
                            UnifiedKind::Exclude => {
                                let input = input::InputBox {
                                    chips: vec![app.groups.excludes[item.id.source_index]
                                        .chip
                                        .clone()],
                                    exclude_mode: true,
                                    ..input::InputBox::default()
                                };
                                preview_lines = app.preview_filter_throttled(&input, preview_limit);
                            }
                        }
                    }
                }
            }
        },
        PickerMode::New | PickerMode::Edit { .. } => match &session.kind {
            PickerKind::Highlight => {
                if !session.draft.is_empty() {
                    labels = app.vocab_match.display_labels().to_vec();
                    styles = vec![theme::muted(); labels.len()];
                }
                preview_lines = preview::preview_highlight_pattern_lines(
                    app,
                    session.draft.as_str(),
                    preview_limit,
                )
                .unwrap_or_default();
                empty_msg = "type a pattern".to_string();
            }
            PickerKind::Filter | PickerKind::Exclude => {
                if let Some(input) = session.input.as_ref() {
                    chips = input.chips.clone();
                    exclude_chips = input.exclude_mode;
                    draft_field = input.draft_field;
                    match input.draft_field {
                        None => {
                            let fields = input.field_candidates();
                            labels = fields.iter().map(|f| f.keyword().to_string()).collect();
                            styles = fields
                                .iter()
                                .map(|f| {
                                    ratatui::style::Style::default().fg(theme::field_color(*f))
                                })
                                .collect();
                            selected = input.field_selected;
                        }
                        Some(field) => {
                            use crate::input::ChipField;
                            labels = match field {
                                ChipField::Tag | ChipField::Pkg | ChipField::Msg => {
                                    app.vocab_match.display_labels().to_vec()
                                }
                                ChipField::Level => level_field_candidates(input.draft.as_str()),
                                ChipField::Pid | ChipField::Tid => vec![],
                            };
                            styles = vec![
                                ratatui::style::Style::default()
                                    .fg(theme::field_color(field));
                                labels.len()
                            ];
                        }
                    }
                    if input.draft_field.is_some() && !input.draft.is_empty() {
                        preview_lines = app.preview_filter_throttled(input, preview_limit);
                    }
                }
                empty_msg = "Enter to add / submit".to_string();
            }
            PickerKind::Preset => {}
            PickerKind::MsgChip { .. } => {
                let visible =
                    PickerSession::filtered_indices(&session.choices, session.draft.as_str());
                labels = visible
                    .iter()
                    .map(|&index| session.choices[index].clone())
                    .collect();
                styles = vec![
                    ratatui::style::Style::default()
                        .fg(theme::field_color(crate::input::ChipField::Msg));
                    labels.len()
                ];
                empty_msg = "type a token".to_string();
            }
            PickerKind::Unified | PickerKind::ActionList { .. } => {}
        },
    }

    let prompt_icon = match &session.kind {
        PickerKind::Preset => Some(theme::GLYPH_GROUP_ON),
        _ => None,
    };

    Some(PickerRenderData {
        title,
        mode,
        text,
        caret,
        match_query,
        chips,
        exclude_chips,
        draft_field,
        labels,
        styles,
        checked,
        actions,
        selected,
        empty_msg,
        preview: preview_lines,
        show_preview,
        preset_preview,
        prompt_icon,
    })
}

/// Visible indices into `app.preset_catalog` for Preset Manage (fuzzy by name).
pub(super) fn preset_visible_indices(app: &App) -> Vec<usize> {
    use crate::picker::PickerSession;
    let session = match app.picker.as_ref() {
        Some(s) => s,
        None => return Vec::new(),
    };
    let labels: Vec<String> = app.preset_catalog.iter().map(|p| p.name.clone()).collect();
    PickerSession::filtered_indices(&labels, session.query.as_str())
}

/// Aggregate Filter → Highlight → Exclude for Manage (bookmark segment removed, F2).
pub(crate) fn unified_picker_items(app: &App) -> Vec<crate::picker::UnifiedItem> {
    use crate::picker::{UnifiedId, UnifiedItem, UnifiedKind};

    let mut items = Vec::new();
    for (source_index, group) in app.groups.groups.iter().enumerate() {
        items.push(UnifiedItem {
            id: UnifiedId {
                kind: UnifiedKind::Filter,
                source_index,
            },
            label: format!("[{}]: {}", UnifiedKind::Filter.tag(), group.label),
            enabled: group.enabled,
        });
    }
    for (source_index, group) in app.highlight_groups.groups.iter().enumerate() {
        items.push(UnifiedItem {
            id: UnifiedId {
                kind: UnifiedKind::Highlight,
                source_index,
            },
            label: format!("[{}]: {}", UnifiedKind::Highlight.tag(), group.pattern),
            enabled: group.enabled,
        });
    }
    for (source_index, entry) in app.groups.excludes.iter().enumerate() {
        let body = format!("{}:{}", entry.chip.field.keyword(), entry.chip.value);
        items.push(UnifiedItem {
            id: UnifiedId {
                kind: UnifiedKind::Exclude,
                source_index,
            },
            label: format!("[{}]: {body}", UnifiedKind::Exclude.tag()),
            enabled: entry.enabled,
        });
    }
    items
}

pub(super) fn unified_visible_ids(app: &App) -> Vec<crate::picker::UnifiedId> {
    use crate::picker::PickerSession;

    let session = match app.picker.as_ref() {
        Some(s) => s,
        None => return Vec::new(),
    };
    // Bookmark / Preset panels key off their own helpers; never enter unified path.
    if matches!(
        session.kind,
        crate::picker::PickerKind::Preset | crate::picker::PickerKind::Highlight
    ) {
        return Vec::new();
    }
    let all = unified_picker_items(app);
    let labels: Vec<String> = all.iter().map(|item| item.label.clone()).collect();
    let visible = PickerSession::filtered_indices(&labels, session.query.as_str());
    visible.iter().map(|&i| all[i].id).collect()
}

pub(super) fn unified_selected_id(app: &App) -> Option<crate::picker::UnifiedId> {
    let session = app.picker.as_ref()?;
    if matches!(
        session.kind,
        crate::picker::PickerKind::Preset | crate::picker::PickerKind::Highlight
    ) {
        return None;
    }
    let ids = unified_visible_ids(app);
    ids.get(session.selected).copied()
}

/// Visible indices into `app.highlight_groups.groups` (storage order).
pub(super) fn highlight_visible_indices(app: &App) -> Vec<usize> {
    use crate::picker::PickerSession;
    let session = match app.picker.as_ref() {
        Some(s) => s,
        None => return Vec::new(),
    };
    let labels: Vec<String> = app
        .highlight_groups
        .groups
        .iter()
        .map(|g| g.pattern.clone())
        .collect();
    PickerSession::filtered_indices(&labels, session.query.as_str())
}

/// Manage create-row query when typed text is not an exact existing pattern.
pub(super) fn highlight_create_query(app: &App) -> Option<&str> {
    use crate::picker::{PickerKind, PickerMode};
    let session = app.picker.as_ref()?;
    if session.kind != PickerKind::Highlight || !matches!(session.mode, PickerMode::Manage) {
        return None;
    }
    let query = session.query.as_str();
    if query.is_empty() {
        return None;
    }
    if app
        .highlight_groups
        .groups
        .iter()
        .any(|g| g.same_pattern_as(query))
    {
        return None;
    }
    Some(query)
}

fn highlight_create_row_label(query: &str) -> String {
    format!("{} {query}", theme::GLYPH_MODE_NEW)
}

pub(super) fn highlight_manage_len(app: &App) -> usize {
    highlight_visible_indices(app).len() + usize::from(highlight_create_query(app).is_some())
}

pub(super) fn highlight_create_row_selected(app: &App) -> bool {
    let selected = app.picker.as_ref().map(|s| s.selected).unwrap_or(0);
    highlight_create_query(app).is_some() && selected == highlight_visible_indices(app).len()
}

pub(super) fn highlight_selected_index(app: &App) -> Option<usize> {
    let vis = highlight_visible_indices(app);
    let session = app.picker.as_ref()?;
    vis.get(session.selected).copied()
}
