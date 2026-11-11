//! Candidate dropdown painting: labels, viewport windowing, input and highlight popups.

use super::{clear_to_canvas, popup_block, ActionKind};
use crate::highlight_model::{HighlightBox, HighlightGroup};
use crate::input::InputBox;
use crate::theme;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, ListState, Paragraph};
use ratatui::Frame;

/// Build label spans with optional fuzzy match coloring (Pattern atoms / multi-range).
/// `checked` changes the selection-marker (prefix) color for Tab multi-select.
fn candidate_label_spans_with_scorer(
    label: &str,
    scorer: &mut crate::fuzzy::FuzzyScorer,
    selected: bool,
    checked: bool,
    base: Style,
    action: ActionKind,
    area_width: u16,
) -> Vec<Span<'static>> {
    use crate::bookmark::fit_label;
    use crate::fuzzy;
    let match_style = theme::candidate_match_style(selected);
    let prefix = if selected || checked {
        theme::candidate_prefix()
    } else {
        " ".repeat(theme::candidate_prefix().chars().count().max(1))
    };
    let prefix_style = if checked {
        theme::candidate_checked_prefix_style().bg(base.bg.unwrap_or(theme::canvas_bg()))
    } else {
        base
    };
    // icon occupies 1 glyph + 1 trailing pad when present.
    let icon_glyph = action.icon();
    let icon_w: u16 = if icon_glyph.is_empty() { 0 } else { 2 };
    let prefix_len = prefix.chars().count() as u16;
    // label budget = area − prefix − icon+pad − 1 trailing pad
    let label_max = (area_width as usize)
        .saturating_sub(prefix_len as usize)
        .saturating_sub(icon_w as usize)
        .saturating_sub(1)
        .max(1);
    let truncated = fit_label(label, label_max);
    let mut spans = vec![Span::styled(prefix, prefix_style)];
    let idxs = scorer.char_indices(&truncated);
    let ranges = fuzzy::char_indices_to_byte_ranges(&truncated, &idxs);
    if ranges.is_empty() {
        spans.push(Span::styled(truncated, base));
    } else {
        let mut cursor = 0usize;
        for (s, e) in ranges {
            if s > cursor {
                spans.push(Span::styled(truncated[cursor..s].to_string(), base));
            }
            if e > s {
                spans.push(Span::styled(truncated[s..e].to_string(), match_style));
            }
            cursor = e;
        }
        if cursor < truncated.len() {
            spans.push(Span::styled(truncated[cursor..].to_string(), base));
        }
    }
    // padding to push the icon flush right, then the icon span.
    let used: usize = spans.iter().map(|sp| sp.content.chars().count()).sum();
    let pad = (area_width as usize)
        .saturating_sub(used)
        .saturating_sub(icon_w as usize);
    if pad > 0 {
        spans.push(Span::raw(" ".repeat(pad)));
    }
    if !icon_glyph.is_empty() {
        spans.push(Span::styled(icon_glyph.to_string(), action.icon_style()));
    }
    spans
}

/// Candidate list skin shared by field popup and Highlight history completion.
/// Selection / match colors and selected-row prefix come from [`theme`].
/// `checked` (same length as `labels`, or empty) marks Tab multi-select rows.
/// When `bordered` is true, draws a rounded popup shell (standalone field/history
/// popups); when false, fills `area` with no chrome (Picker left pane already
/// has an outer shell).
pub fn render_candidate_list(
    title: &str,
    labels: &[String],
    styles: &[Style],
    checked: &[bool],
    actions: &[ActionKind],
    selected: usize,
    empty_msg: &str,
    query: &str,
    frame: &mut Frame,
    area: Rect,
    bordered: bool,
) {
    let inner = if bordered {
        clear_to_canvas(frame, area);
        let block = popup_block(title);
        let inner = block.inner(area);
        frame.render_widget(block, area);
        inner
    } else {
        area
    };
    if labels.is_empty() {
        frame.render_widget(
            Paragraph::new(Span::styled(
                empty_msg,
                Style::default().add_modifier(Modifier::DIM),
            )),
            inner,
        );
        return;
    }
    let sel = selected.min(labels.len() - 1);
    // ViewportPaint: only build ListItems (+ fuzzy highlight) for visible rows.
    let n = labels.len();
    let (offset, end) = candidate_viewport_range(n, sel, inner.height as usize);
    // Reuse one FuzzyScorer across the viewport (same query × many labels).
    let mut paint_scorer = crate::fuzzy::FuzzyScorer::new(query);
    let items: Vec<ListItem> = (offset..end)
        .map(|i| {
            let label = &labels[i];
            let is_sel = i == sel;
            let is_checked = checked.get(i).copied().unwrap_or(false);
            let mut base = if is_sel {
                theme::candidate_selected_style()
            } else {
                theme::candidate_unselected_style()
            };
            // Kind/field-colored candidates keep their fg when not selected.
            if !is_sel {
                if let Some(style) = styles.get(i) {
                    if let Some(fg) = style.fg {
                        base = base.fg(fg);
                    }
                }
            }
            ListItem::new(Line::from(candidate_label_spans_with_scorer(
                label,
                &mut paint_scorer,
                is_sel,
                is_checked,
                base,
                actions.get(i).copied().unwrap_or(ActionKind::None),
                inner.width,
            )))
            .style(base)
        })
        .collect();
    let list = List::new(items)
        .highlight_style(Style::default())
        .highlight_symbol("");
    let mut state = ListState::default();
    state.select(Some(sel.saturating_sub(offset)));
    frame.render_stateful_widget(list, inner, &mut state);
}

/// Visible `[offset, end)` window for a candidate list (ViewportPaint SLO).
pub fn candidate_viewport_range(n: usize, selected: usize, view_h: usize) -> (usize, usize) {
    if n == 0 {
        return (0, 0);
    }
    let view_h = view_h.max(1);
    let sel = selected.min(n - 1);
    let offset = sel
        .saturating_sub(view_h.saturating_sub(1) / 2)
        .min(n.saturating_sub(view_h));
    let end = (offset + view_h).min(n);
    (offset, end)
}

/// Search history-chip candidates — same shell as Input field popup.
pub fn render_highlight_popup(
    search: &HighlightBox,
    groups: &[HighlightGroup],
    frame: &mut Frame,
    area: Rect,
) {
    let candidates = search.candidate_indices(groups);
    let n = candidates.len().min(6);
    let labels: Vec<String> = candidates
        .iter()
        .take(n)
        .map(|&i| groups[i].pattern.clone())
        .collect();
    // Color by each group's global enabled-pattern index for consistency
    // with strip pills; fall back to dim if disabled.
    let mut color_idx = 0usize;
    let mut group_color: Vec<Option<usize>> = Vec::with_capacity(groups.len());
    for g in groups {
        if g.enabled {
            group_color.push(Some(color_idx));
            color_idx += 1;
        } else {
            group_color.push(None);
        }
    }
    let styles: Vec<Style> = candidates
        .iter()
        .take(n)
        .map(|&i| match group_color[i] {
            Some(idx) => theme::highlight_style(idx),
            None => theme::disabled_chip_style(),
        })
        .collect();
    let selected = if n == 0 {
        0
    } else {
        search.selected.min(n - 1)
    };
    render_candidate_list(
        "History",
        &labels,
        &styles,
        &[],
        &[],
        selected,
        "no history",
        &search.draft,
        frame,
        area,
        true,
    );
}

pub fn render_popup(input: &InputBox, frame: &mut Frame, area: Rect) {
    if !input.field_popup_visible() {
        return;
    }
    let matches = input.field_candidates();
    let labels: Vec<String> = matches.iter().map(|f| f.keyword().to_string()).collect();
    let styles: Vec<Style> = matches
        .iter()
        .map(|&f| Style::default().fg(theme::field_color(f)))
        .collect();
    let selected = if matches.is_empty() {
        0
    } else {
        input.field_selected.min(matches.len() - 1)
    };
    render_candidate_list(
        "Fields",
        &labels,
        &styles,
        &[],
        &[],
        selected,
        "no match",
        &input.draft,
        frame,
        area,
        true,
    );
}
