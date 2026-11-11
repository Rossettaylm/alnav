//! Command palette renderer.

use crate::app::App;
use crate::theme;
use crate::ui::{
    candidate_viewport_range, clear_to_canvas, command_palette_width, editable_text_spans,
    render_modal_shell_glyph, rounded_block, search_modal_height, stack_below_rect_gapped,
    top_modal_rect,
};
use ratatui::layout::{Position, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, ListState, Paragraph};
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;

/// VS Code-style command palette: input shell, optional candidate dropdown.
pub fn render_command_palette(app: &App, frame: &mut Frame, frame_area: Rect) -> Option<Position> {
    let Some(palette) = app.command_palette.as_ref() else {
        return None;
    };
    let width = command_palette_width(frame_area);
    let input_h = search_modal_height();
    let input_area = top_modal_rect(frame_area, width, input_h);
    let inner = render_modal_shell_glyph(
        theme::GLYPH_TITLE_PALETTE,
        "Command Palette",
        frame,
        input_area,
    );
    let query = palette.query.as_str();
    let (spans, caret_col) = editable_text_spans(query, palette.query.cursor(), Some(inner.width));
    frame.render_widget(Paragraph::new(Line::from(spans)), inner);
    let cursor = Some(Position {
        x: inner
            .x
            .saturating_add(caret_col.min(inner.width.saturating_sub(1))),
        y: inner.y,
    });

    if query.is_empty() {
        return cursor;
    }

    let hits = crate::action::filtered_catalog(app, query);
    let list_h = if hits.is_empty() {
        3
    } else {
        (hits.len().min(crate::command_palette::PALETTE_VISIBLE_ROWS) as u16).saturating_add(2)
    };
    let list_area = stack_below_rect_gapped(input_area, frame_area, list_h.max(3));
    if list_area.height == 0 {
        return cursor;
    }
    clear_to_canvas(frame, list_area);
    let block = rounded_block(
        theme::plain_title(theme::GLYPH_TITLE_PALETTE, "", true),
        true,
    );
    let list_inner = block.inner(list_area);
    frame.render_widget(block, list_area);
    clear_to_canvas(frame, list_inner);
    if list_inner.width == 0 || list_inner.height == 0 {
        return cursor;
    }

    if hits.is_empty() {
        frame.render_widget(
            Paragraph::new(Span::styled(
                "No matching commands",
                theme::candidate_unselected_style(),
            )),
            list_inner,
        );
        return cursor;
    }

    let n = hits.len();
    let sel = palette.selected.min(n - 1);
    let view_h = list_inner.height as usize;
    let (offset, end) = candidate_viewport_range(n, sel, view_h);
    let mut scorer = crate::fuzzy::FuzzyScorer::new(query);
    let items: Vec<ListItem> = (offset..end)
        .map(|i| {
            let item = &hits[i];
            let is_sel = i == sel;
            let mut base = if is_sel {
                theme::candidate_selected_style()
            } else {
                theme::candidate_unselected_style()
            };
            let line = palette_row_spans(item, &mut scorer, is_sel, list_inner.width, &mut base);
            ListItem::new(Line::from(line)).style(base)
        })
        .collect();
    let list = List::new(items)
        .highlight_style(Style::default())
        .highlight_symbol("");
    let mut state = ListState::default();
    state.select(Some(sel.saturating_sub(offset)));
    frame.render_stateful_widget(list, list_inner, &mut state);
    cursor
}

fn palette_row_spans(
    item: &crate::action::PaletteItem,
    scorer: &mut crate::fuzzy::FuzzyScorer,
    selected: bool,
    area_width: u16,
    base: &mut Style,
) -> Vec<Span<'static>> {
    use crate::bookmark::fit_label;
    let hint = &item.key_hint;
    let hint_w = UnicodeWidthStr::width(hint.as_str()) as u16;
    let icon_w: u16 = 2; // glyph + space
    let gap: u16 = if hint.is_empty() { 0 } else { 1 };
    let title_max = (area_width as usize)
        .saturating_sub(icon_w as usize)
        .saturating_sub(hint_w as usize)
        .saturating_sub(gap as usize)
        .max(1);
    let truncated = fit_label(item.title, title_max);
    let match_style = theme::candidate_match_style(selected);
    let idxs = scorer.char_indices(&truncated);
    let ranges = crate::fuzzy::char_indices_to_byte_ranges(&truncated, &idxs);
    let mut spans = vec![Span::styled(format!("{} ", item.icon), *base)];
    if ranges.is_empty() {
        spans.push(Span::styled(truncated.clone(), *base));
    } else {
        let mut cursor = 0usize;
        for (s, e) in ranges {
            if s > cursor {
                spans.push(Span::styled(truncated[cursor..s].to_string(), *base));
            }
            if e > s {
                spans.push(Span::styled(truncated[s..e].to_string(), match_style));
            }
            cursor = e;
        }
        if cursor < truncated.len() {
            spans.push(Span::styled(truncated[cursor..].to_string(), *base));
        }
    }
    let title_w = UnicodeWidthStr::width(truncated.as_str()) as u16;
    let used = icon_w.saturating_add(title_w);
    let pad = area_width.saturating_sub(used).saturating_sub(hint_w);
    if pad > 0 {
        spans.push(Span::raw(" ".repeat(pad as usize)));
    }
    if !hint.is_empty() {
        spans.push(Span::styled(hint.clone(), theme::palette_keyhint_style()));
    }
    spans
}
