//! Picker surface: search line, left/right panes, previews, preset and confirm dialogs.

use super::entry::{render_entry_line_single, PaintPattern};
use super::strips::{
    committed_chip_lines, committed_chip_rows, exclude_entry_spans, filter_group_spans,
    flow_wrap_groups, highlight_group_spans,
};
use super::text::spans_display_width;
use super::{
    centered_modal_rect, clear_to_canvas, detail_field_lines, editable_text_spans, modal_width,
    picker_frame_rect, picker_left_stack, render_candidate_list, render_modal_shell, rounded_block,
    split_picker_lr_gapped, ActionKind, PICKER_SEARCH_HEIGHT,
};
use crate::input::ChipField;
use crate::theme;
use ratatui::layout::{Alignment, Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, Paragraph};
use ratatui::Frame;

/// Horizontal padding between the search border and its content.
const PICKER_SEARCH_HORIZONTAL_PADDING: u16 = 1;

/// H1 Preview window: LogList-styled single-line hits (Filter / Highlight / Search).
pub fn render_preview(
    title: &str,
    lines: &[crate::preview::PreviewHit],
    placeholder: &str,
    frame: &mut Frame,
    area: Rect,
) {
    if area.height == 0 {
        return;
    }
    let inner = render_modal_shell(title, frame, area);
    if lines.is_empty() {
        frame.render_widget(
            Paragraph::new(Span::styled(
                placeholder.to_string(),
                theme::preview_placeholder_style(),
            )),
            inner,
        );
        return;
    }
    let area_width = inner.width as usize;
    let items: Vec<ListItem> = lines
        .iter()
        .map(|hit| {
            let patterns: Vec<PaintPattern<'_>> = hit
                .pattern
                .as_deref()
                .map(|p| vec![(p, 0usize, true)])
                .unwrap_or_default();
            ListItem::new(render_entry_line_single(&hit.row, &patterns, area_width))
        })
        .collect();
    frame.render_widget(List::new(items), inner);
}

/// fzf left-pane committed pills plus search prompt:
/// mode icon (search / `＋` / `✎`) + optional `field:` + draft.
/// Returns hardware cursor position for the draft caret.
///
/// `chip_rows` is the height reserved above the rounded input for wrapped
/// committed chips (must match [`picker_left_stack`]).
pub fn render_picker_search_line(
    mode: &crate::picker::PickerMode,
    text: &str,
    caret: usize,
    chips: &[crate::input::Chip],
    exclude_chips: bool,
    draft_field: Option<ChipField>,
    // Override mode icon (e.g. bookmark glyph for Bookmark picker).
    prompt_icon: Option<&'static str>,
    chip_rows: u16,
    frame: &mut Frame,
    area: Rect,
) -> Option<Position> {
    if area.height == 0 {
        return None;
    }

    let chip_h = chip_rows.min(
        area.height
            .saturating_sub(PICKER_SEARCH_HEIGHT.min(area.height)),
    );
    if chip_h > 0 && !chips.is_empty() {
        let chip_area = Rect {
            x: area.x,
            y: area.y,
            width: area.width,
            height: chip_h,
        };
        let lines = committed_chip_lines(chips, exclude_chips, chip_area.width);
        frame.render_widget(Paragraph::new(lines), chip_area);
    }

    let input_area = Rect {
        x: area.x,
        y: area.y.saturating_add(chip_h),
        width: area.width,
        height: area.height.saturating_sub(chip_h),
    };
    if input_area.height == 0 {
        return None;
    }

    let block = rounded_block(Line::from(""), true);
    let inner = block.inner(input_area);
    frame.render_widget(block, input_area);
    if inner.width == 0 || inner.height == 0 {
        return None;
    }

    let padding = PICKER_SEARCH_HORIZONTAL_PADDING.min(inner.width / 2);
    let content = Rect {
        x: inner.x.saturating_add(padding),
        y: inner.y,
        width: inner.width.saturating_sub(padding.saturating_mul(2)),
        height: inner.height,
    };
    if content.width == 0 {
        return None;
    }

    let mut prompt_spans = vec![match prompt_icon {
        Some(icon) => theme::picker_prompt_prefix(icon),
        None => theme::picker_mode_prefix(mode),
    }];
    if let Some(field) = draft_field {
        prompt_spans.push(Span::styled(
            format!("{} {}:", theme::field_icon(field), field.keyword()),
            Style::reset().fg(theme::field_color(field)),
        ));
    }
    let prefix_w = spans_display_width(&prompt_spans);
    let draft_max = (content.width as usize).saturating_sub(prefix_w) as u16;
    let (draft_spans, caret_col) = editable_text_spans(text, caret, Some(draft_max));
    prompt_spans.extend(draft_spans);
    frame.render_widget(Paragraph::new(Line::from(prompt_spans)), content);
    let x_off = (prefix_w as u16).saturating_add(caret_col);
    // `x_off == content.width` places the caret after the last visible char
    // (into the right horizontal padding).
    Some(Position {
        x: content.x.saturating_add(x_off.min(content.width)),
        y: content.y,
    })
}

/// Right-pane content for [`render_picker`].
pub enum PickerRightPane<'a> {
    /// Filter/Highlight-style sampled log hits.
    Hits(&'a [crate::preview::PreviewHit]),
    /// Preset panel: chip-strip style Filter → Exclude → Highlight.
    ChipRules(&'a [Line<'static>]),
    /// Open-file head preview (plain text lines).
    PlainLines(&'a [Line<'static>]),
}

/// fzf-style picker shell: left candidates + bottom search, right Preview.
/// Returns hardware cursor position for the search-line caret.
pub fn render_picker(
    title: &str,
    mode: &crate::picker::PickerMode,
    search_text: &str,
    caret: usize,
    match_query: &str,
    chips: &[crate::input::Chip],
    exclude_chips: bool,
    draft_field: Option<ChipField>,
    labels: &[String],
    styles: &[Style],
    checked: &[bool],
    actions: &[ActionKind],
    selected: usize,
    empty_msg: &str,
    right_pane: PickerRightPane<'_>,
    left_ratio: f32,
    show_preview: bool,
    prompt_icon: Option<&'static str>,
    frame: &mut Frame,
    frame_area: Rect,
) -> Option<Position> {
    let picker_area = picker_frame_rect(frame_area, show_preview);
    clear_to_canvas(frame, picker_area);

    // Honor caller `left_ratio` (config `picker_left_ratio`) for both preview and
    // compact pickers. Preview used to hard-code 0.3, which crushed Open-file labels.
    let split_ratio = left_ratio;
    let (left, right) = if show_preview {
        let (l, r) = split_picker_lr_gapped(picker_area, split_ratio);
        (l, Some(r))
    } else {
        (picker_area, None)
    };
    let left_inner = render_modal_shell(title, frame, left);
    let chip_rows = committed_chip_rows(chips, exclude_chips, left_inner.width, left_inner.height);
    let (candidates_area, search_area) = picker_left_stack(left_inner, chip_rows);

    if candidates_area.height > 0 {
        render_candidate_list(
            "list",
            labels,
            styles,
            checked,
            actions,
            selected,
            empty_msg,
            match_query,
            frame,
            candidates_area,
            false,
        );
    }
    let cursor = render_picker_search_line(
        mode,
        search_text,
        caret,
        chips,
        exclude_chips,
        draft_field,
        prompt_icon,
        chip_rows,
        frame,
        search_area,
    );
    if let Some(right) = right {
        match right_pane {
            PickerRightPane::Hits(preview_lines) => {
                render_preview("Preview", preview_lines, "no preview", frame, right);
            }
            PickerRightPane::ChipRules(lines) => {
                render_preset_rules_preview(lines, frame, right);
            }
            PickerRightPane::PlainLines(lines) => {
                render_plain_preview(lines, frame, right);
            }
        }
    }
    cursor
}

fn render_plain_preview(lines: &[Line<'static>], frame: &mut Frame, area: Rect) {
    let inner = render_modal_shell("Path", frame, area);
    if lines.is_empty() {
        frame.render_widget(
            Paragraph::new(Span::styled(
                "no selection",
                theme::preview_placeholder_style(),
            )),
            inner,
        );
        return;
    }
    frame.render_widget(
        Paragraph::new(lines.to_vec()).wrap(ratatui::widgets::Wrap { trim: false }),
        inner,
    );
}

/// Preset Preview: stacked Filter / Exclude / Highlight chip rows (strip style).
pub fn render_preset_rules_preview(lines: &[Line<'static>], frame: &mut Frame, area: Rect) {
    if area.height == 0 {
        return;
    }
    let inner = render_modal_shell("Preview", frame, area);
    if lines.is_empty() {
        frame.render_widget(
            Paragraph::new(Span::styled("no rules", theme::preview_placeholder_style())),
            inner,
        );
        return;
    }
    frame.render_widget(Paragraph::new(lines.to_vec()), inner);
}

/// Build Preview lines for a preset (Filter → Exclude → Highlight), strip-like pills.
pub fn preset_preview_lines(preset: &crate::preset::Preset, width: u16) -> Vec<Line<'static>> {
    let width = width.max(1) as usize;
    let mut out = Vec::new();
    if let Ok((groups, highlights)) = crate::preset::materialize(preset) {
        if !groups.groups.is_empty() {
            out.push(Line::from(Span::styled(
                "Filter",
                theme::muted().add_modifier(Modifier::BOLD),
            )));
            let groups_spans: Vec<Vec<Span<'static>>> = groups
                .groups
                .iter()
                .map(|g| filter_group_spans(g, false))
                .collect();
            out.extend(flow_wrap_groups(groups_spans, width as u16));
        }
        if !groups.excludes.is_empty() {
            if !out.is_empty() {
                out.push(Line::from(""));
            }
            out.push(Line::from(Span::styled(
                "Exclude",
                theme::muted().add_modifier(Modifier::BOLD),
            )));
            let groups_spans: Vec<Vec<Span<'static>>> = groups
                .excludes
                .iter()
                .map(|e| exclude_entry_spans(e, false))
                .collect();
            out.extend(flow_wrap_groups(groups_spans, width as u16));
        }
        if !highlights.groups.is_empty() {
            if !out.is_empty() {
                out.push(Line::from(""));
            }
            out.push(Line::from(Span::styled(
                "Highlight",
                theme::muted().add_modifier(Modifier::BOLD),
            )));
            let mut color_idx = 0usize;
            let groups_spans: Vec<Vec<Span<'static>>> = highlights
                .groups
                .iter()
                .map(|g| {
                    let idx = if g.enabled {
                        let c = color_idx;
                        color_idx += 1;
                        c
                    } else {
                        0
                    };
                    highlight_group_spans(g, idx, false, false)
                })
                .collect();
            out.extend(flow_wrap_groups(groups_spans, width as u16));
        }
    }
    out
}

/// Save / rename preset name dialog (no candidates, no preview).
/// Returns hardware cursor position for the draft caret.
pub fn render_preset_name_dialog(
    dialog: &crate::preset::PresetNameDialog,
    frame: &mut Frame,
    frame_area: Rect,
) -> Option<Position> {
    use crate::preset::PresetNamePurpose;

    let title = match &dialog.purpose {
        PresetNamePurpose::Save => "Save preset",
        PresetNamePurpose::Rename { .. } => "Rename preset",
    };
    let modal_w = modal_width(frame_area.width).min(48);
    let area = centered_modal_rect(frame_area, modal_w, 5);
    clear_to_canvas(frame, area);
    let inner = render_modal_shell(title, frame, area);
    if dialog.confirm_overwrite {
        let text = vec![
            Line::from(Span::styled(
                format!("Overwrite '{}'?", dialog.field.as_str()),
                Style::default().add_modifier(Modifier::BOLD),
            ))
            .alignment(Alignment::Center),
            Line::from(Span::styled(
                "y/Enter confirm  n/Esc cancel",
                theme::context_help_style(),
            ))
            .alignment(Alignment::Center),
        ];
        frame.render_widget(Paragraph::new(text).alignment(Alignment::Center), inner);
        return None;
    }
    let mut spans = vec![theme::picker_mode_prefix(&crate::picker::PickerMode::New)];
    let prefix_w = spans_display_width(&spans) as u16;
    let draft_max = inner.width.saturating_sub(prefix_w);
    let (draft_spans, caret_col) = editable_text_spans(
        dialog.field.as_str(),
        dialog.field.cursor(),
        Some(draft_max),
    );
    spans.extend(draft_spans);
    frame.render_widget(Paragraph::new(Line::from(spans)), inner);
    Some(Position {
        x: inner
            .x
            .saturating_add(prefix_w.saturating_add(caret_col).min(inner.width)),
        y: inner.y,
    })
}

/// Bookmark picker right pane: same Fields shell/content as LogList `p`.
pub fn render_picker_detail(row: Option<&crate::model::EntryRow>, frame: &mut Frame, area: Rect) {
    if area.height == 0 {
        return;
    }
    let inner = render_modal_shell("Detail", frame, area);
    let lines = if row.is_some() {
        detail_field_lines(row, inner.width)
    } else {
        vec![Line::from(Span::styled(
            "row gone".to_string(),
            theme::preview_placeholder_style(),
        ))]
    };
    let max_rows = inner.height as usize;
    let shown: Vec<Line<'static>> = lines.into_iter().take(max_rows).collect();
    frame.render_widget(Paragraph::new(shown), inner);
}

/// Destructive confirm, overlaid at the frame center.
pub(super) fn confirm_dialog_question(confirm: &crate::picker::ConfirmKind) -> String {
    match confirm {
        crate::picker::ConfirmKind::DeleteMany { items } => {
            if items.len() == 1 {
                "Delete selected?".to_string()
            } else {
                format!("Delete {} items?", items.len())
            }
        }
        crate::picker::ConfirmKind::DeletePreset { name } => format!("Delete preset '{name}'?"),
        crate::picker::ConfirmKind::ClearAll => "Clear all rules?".to_string(),
    }
}

pub fn render_confirm_dialog(
    confirm: &crate::picker::ConfirmKind,
    frame: &mut Frame,
    frame_area: Rect,
) {
    let question = confirm_dialog_question(confirm);
    let width = 34.min(frame_area.width).max(1);
    let height = 5.min(frame_area.height).max(1);
    let area = centered_modal_rect(frame_area, width, height);
    let inner = render_modal_shell("Confirm", frame, area);
    let text = vec![
        Line::from(Span::styled(
            question,
            Style::default().add_modifier(Modifier::BOLD),
        ))
        .alignment(Alignment::Center),
        Line::from(Span::styled(
            "y/Enter confirm  n/Esc cancel",
            theme::context_help_style(),
        ))
        .alignment(Alignment::Center),
    ];
    frame.render_widget(Paragraph::new(text).alignment(Alignment::Center), inner);
}
