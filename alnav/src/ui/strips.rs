//! Filter / exclude / highlight chip strips: pill spans, flow wrapping and strip heights.

use super::{divider_block, PICKER_SEARCH_HEIGHT, PILL_GAP};
use crate::app::{App, Focus};
use crate::filter_model::Group;
use crate::highlight_model::HighlightGroup;
use crate::theme;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

/// Horizontal gap (columns) between chip groups on the same wrap row.
const CHIP_GROUP_GAP: u16 = 1;

/// Gap between the selection marker and the group's pills.
const DOT_PILL_GAP: u16 = 1;

fn group_dot_span(enabled: bool, selected: bool) -> Span<'static> {
    let dot = if enabled {
        theme::GLYPH_GROUP_ON
    } else {
        theme::GLYPH_GROUP_OFF
    };
    // Selection uses the same Magenta accent as region selection frames;
    // kept to one cell so the strip can stay a single content row tall.
    let style = if selected {
        theme::chip_group_border_style(true).add_modifier(Modifier::BOLD)
    } else if !enabled {
        theme::disabled_chip_style()
    } else {
        Style::default().add_modifier(Modifier::DIM)
    };
    Span::styled(dot.to_string(), style)
}

pub(super) fn filter_group_spans(g: &Group, selected: bool) -> Vec<Span<'static>> {
    let mut spans = vec![group_dot_span(g.enabled, selected)];
    spans.push(Span::raw(" ".repeat(DOT_PILL_GAP as usize)));
    if g.chips.is_empty() {
        let style = if !g.enabled {
            theme::disabled_chip_style()
        } else {
            Style::default()
        };
        spans.push(Span::styled(format!(" {} ", g.label), style));
    } else {
        for (i, chip) in g.chips.iter().enumerate() {
            if i > 0 {
                spans.push(Span::raw(" ".repeat(PILL_GAP as usize)));
            }
            let pill = theme::chip_pill_spans(chip.field, &chip.value, !g.enabled);
            spans.extend(pill);
        }
    }
    spans
}

pub(super) fn highlight_group_spans(
    g: &HighlightGroup,
    color_idx: usize,
    selected: bool,
    active_global: bool,
) -> Vec<Span<'static>> {
    let mut spans = vec![group_dot_span(g.enabled, selected)];
    spans.push(Span::raw(" ".repeat(DOT_PILL_GAP as usize)));
    let pill = theme::highlight_pill_spans(&g.pattern, color_idx, !g.enabled, active_global);
    spans.extend(pill);
    spans
}

pub(super) fn exclude_entry_spans(
    e: &crate::filter_model::ExcludeEntry,
    selected: bool,
) -> Vec<Span<'static>> {
    let mut spans = vec![group_dot_span(e.enabled, selected)];
    spans.push(Span::raw(" ".repeat(DOT_PILL_GAP as usize)));
    let pill = theme::exclude_pill_spans(e.chip.field, &e.chip.value, !e.enabled);
    spans.extend(pill);
    spans
}

pub(super) fn span_width(span: &Span<'_>) -> usize {
    span.content.chars().count()
}

fn wrap_spans(spans: Vec<Span<'static>>, width: usize) -> Vec<Line<'static>> {
    let width = width.max(1);
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut current: Vec<Span<'static>> = Vec::new();
    let mut used = 0usize;

    for span in spans {
        let w = span_width(&span);
        if w > width {
            if !current.is_empty() {
                lines.push(Line::from(std::mem::take(&mut current)));
                used = 0;
            }
            let text = span.content.as_ref().to_string();
            let style = span.style;
            let chars: Vec<char> = text.chars().collect();
            let mut i = 0;
            while i < chars.len() {
                let end = (i + width).min(chars.len());
                let chunk: String = chars[i..end].iter().collect();
                lines.push(Line::from(Span::styled(chunk, style)));
                i = end;
            }
            continue;
        }
        if !current.is_empty() && used + w > width {
            lines.push(Line::from(std::mem::take(&mut current)));
            used = 0;
        }
        used += w;
        current.push(span);
    }
    if !current.is_empty() || lines.is_empty() {
        lines.push(Line::from(current));
    }
    lines
}

pub(super) fn flow_wrap_groups(groups: Vec<Vec<Span<'static>>>, width: u16) -> Vec<Line<'static>> {
    let width = width.max(1) as usize;
    let mut out: Vec<Line<'static>> = Vec::new();
    let mut row_spans: Vec<Span<'static>> = Vec::new();
    let mut used = 0usize;

    for group in groups {
        let group_w: usize = group.iter().map(span_width).sum();
        if group_w > width {
            if !row_spans.is_empty() {
                out.push(Line::from(std::mem::take(&mut row_spans)));
                used = 0;
            }
            out.extend(wrap_spans(group, width));
            continue;
        }
        let need = if row_spans.is_empty() {
            group_w
        } else {
            CHIP_GROUP_GAP as usize + group_w
        };
        if !row_spans.is_empty() && used + need > width {
            out.push(Line::from(std::mem::take(&mut row_spans)));
            used = 0;
        }
        if !row_spans.is_empty() {
            row_spans.push(Span::raw(" ".repeat(CHIP_GROUP_GAP as usize)));
            used += CHIP_GROUP_GAP as usize;
        }
        used += group_w;
        row_spans.extend(group);
    }
    if !row_spans.is_empty() {
        out.push(Line::from(row_spans));
    }
    out
}

fn filter_strip_lines(app: &App, inner_width: u16) -> Vec<Line<'static>> {
    let active = app.focus == Focus::ChipStrip;
    let groups: Vec<Vec<Span<'static>>> = app
        .groups
        .groups
        .iter()
        .enumerate()
        .map(|(i, g)| filter_group_spans(g, i == app.group_cursor && active))
        .collect();
    flow_wrap_groups(groups, inner_width)
}

fn highlight_strip_lines(app: &App, inner_width: u16) -> Vec<Line<'static>> {
    let active = app.focus == Focus::HighlightStrip;
    let mut color_idx = 0usize;
    let groups: Vec<Vec<Span<'static>>> = app
        .highlight_groups
        .groups
        .iter()
        .enumerate()
        .map(|(i, g)| {
            let idx = if g.enabled {
                let c = color_idx;
                color_idx += 1;
                c
            } else {
                0
            };
            highlight_group_spans(
                g,
                idx,
                i == app.highlight_cursor && active,
                Some(i) == app.active_highlight,
            )
        })
        .collect();
    flow_wrap_groups(groups, inner_width)
}

fn exclude_strip_lines(app: &App, inner_width: u16) -> Vec<Line<'static>> {
    let active = app.focus == Focus::ExcludeStrip;
    let groups: Vec<Vec<Span<'static>>> = app
        .groups
        .excludes
        .iter()
        .enumerate()
        .map(|(i, e)| exclude_entry_spans(e, i == app.exclude_cursor && active))
        .collect();
    flow_wrap_groups(groups, inner_width)
}

/// Strip height: `0` when empty, else `2` (rounded region chrome) + content
/// rows. Content is a single terminal row per wrap line — nested per-chip
/// `Block`s need 3 rows each and made the strip ~2× taller than a cell's
/// visual proportions allow.
pub fn filter_strip_height(app: &App, outer_width: u16) -> u16 {
    if app.groups.groups.is_empty() {
        return 0;
    }
    let inner = outer_width.saturating_sub(2);
    let rows = filter_strip_lines(app, inner).len().max(1) as u16;
    rows.saturating_add(2)
}

/// Same rules as [`filter_strip_height`] for the Exclude strip (H9).
pub fn exclude_strip_height(app: &App, outer_width: u16) -> u16 {
    if app.groups.excludes.is_empty() {
        return 0;
    }
    let inner = outer_width.saturating_sub(2);
    let rows = exclude_strip_lines(app, inner).len().max(1) as u16;
    rows.saturating_add(2)
}

/// Same rules as [`filter_strip_height`] for the Search strip.
pub fn highlight_strip_height(app: &App, outer_width: u16) -> u16 {
    if app.highlight_groups.groups.is_empty() {
        return 0;
    }
    let inner = outer_width.saturating_sub(2);
    let rows = highlight_strip_lines(app, inner).len().max(1) as u16;
    rows.saturating_add(2)
}

pub fn render_chip_strip(app: &App, frame: &mut Frame, area: Rect) {
    if area.height == 0 {
        return;
    }
    let active = app.focus == Focus::ChipStrip;
    let block = divider_block(theme::numbered_title(1, "Filter", active), active);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(Paragraph::new(filter_strip_lines(app, inner.width)), inner);
}

pub fn render_exclude_chip_strip(app: &App, frame: &mut Frame, area: Rect) {
    if area.height == 0 {
        return;
    }
    let active = app.focus == Focus::ExcludeStrip;
    let block = divider_block(theme::numbered_title(2, "Exclude", active), active);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(Paragraph::new(exclude_strip_lines(app, inner.width)), inner);
}

pub fn render_highlight_chip_strip(app: &App, frame: &mut Frame, area: Rect) {
    if area.height == 0 {
        return;
    }
    let active = app.focus == Focus::HighlightStrip;
    let block = divider_block(theme::numbered_title(3, "Highlight", active), active);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(
        Paragraph::new(highlight_strip_lines(app, inner.width)),
        inner,
    );
}

/// Wrap committed pills to `width` (each chip is an atomic flow group).
pub(super) fn committed_chip_lines(
    chips: &[crate::input::Chip],
    exclude_mode: bool,
    width: u16,
) -> Vec<Line<'static>> {
    if chips.is_empty() {
        return Vec::new();
    }
    let groups: Vec<Vec<Span<'static>>> = chips
        .iter()
        .map(|chip| {
            if exclude_mode {
                theme::exclude_pill_spans(chip.field, &chip.value, false)
            } else {
                theme::chip_pill_spans(chip.field, &chip.value, false)
            }
        })
        .collect();
    flow_wrap_groups(groups, width)
}

/// Rows needed for committed chips in the picker search band.
/// Caps so at least one candidate row remains above the search input.
pub(super) fn committed_chip_rows(
    chips: &[crate::input::Chip],
    exclude_mode: bool,
    width: u16,
    left_height: u16,
) -> u16 {
    if chips.is_empty() {
        return 0;
    }
    let rows = committed_chip_lines(chips, exclude_mode, width)
        .len()
        .max(1) as u16;
    let max_chip = left_height.saturating_sub(PICKER_SEARCH_HEIGHT.saturating_add(1));
    rows.min(max_chip)
}
