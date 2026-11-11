//! Input and highlight modal renderers (chip input box with hardware cursor).

use crate::app::Mode;
use crate::highlight_model::HighlightBox;
use crate::input::InputBox;
use crate::theme;
use crate::ui::strips::committed_chip_spans;
use crate::ui::text::spans_display_width;
#[cfg(test)]
use crate::ui::{clear_to_canvas, divider_block};
use crate::ui::{editable_text_spans, render_modal_shell, PILL_GAP};
use ratatui::layout::{Position, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

/// Build Input draft line spans; returns caret column within `inner` when editing.
fn input_content_spans(
    input: &InputBox,
    show_caret: bool,
    max_width: Option<u16>,
) -> (Vec<Span<'static>>, Option<u16>) {
    let mut spans = committed_chip_spans(&input.chips, input.exclude_mode);
    // Gap + reset after pills so draft never sits inside the pill fill.
    if !input.chips.is_empty() {
        spans.push(Span::styled(" ".repeat(PILL_GAP as usize), Style::reset()));
    }
    if let Some(field) = input.draft_field {
        spans.push(Span::styled(
            format!("{} {}:", theme::field_icon(field), field.keyword()),
            Style::reset().fg(theme::field_color(field)),
        ));
    }
    if show_caret {
        let prefix_w = spans_display_width(&spans);
        let draft_max = max_width.map(|w| (w as usize).saturating_sub(prefix_w) as u16);
        let (draft_spans, caret_col) =
            editable_text_spans(input.draft.as_str(), input.draft.cursor(), draft_max);
        spans.extend(draft_spans);
        let col = (prefix_w as u16).saturating_add(caret_col);
        (spans, Some(col))
    } else {
        spans.push(Span::styled(input.draft.to_string(), Style::reset()));
        (spans, None)
    }
}

/// Centered Input modal (visible while `Focus::Input`).
/// Returns hardware cursor position when in Insert mode.
pub fn render_input_modal(
    input: &InputBox,
    mode: Mode,
    frame: &mut Frame,
    area: Rect,
) -> Option<Position> {
    let title = if input.exclude_mode {
        "Input !"
    } else {
        "Input"
    };
    let inner = render_modal_shell(title, frame, area);
    let (spans, caret_col) = input_content_spans(input, mode == Mode::Insert, Some(inner.width));
    frame.render_widget(Paragraph::new(Line::from(spans)), inner);
    caret_col.map(|col| Position {
        // `col == width` means after the last visible char (into right padding/border).
        x: inner.x.saturating_add(col.min(inner.width)),
        y: inner.y,
    })
}

/// Legacy single-row Input render kept for unit tests that draw into a fixed area.
#[cfg(test)]
pub fn render_input_box(
    input: &InputBox,
    mode: Mode,
    focused: bool,
    frame: &mut Frame,
    area: Rect,
) -> Option<Position> {
    let block = divider_block(theme::numbered_title(5, "Input", focused), focused);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    clear_to_canvas(frame, inner);
    let (spans, caret_col) = input_content_spans(input, mode == Mode::Insert, Some(inner.width));
    frame.render_widget(Paragraph::new(Line::from(spans)), inner);
    caret_col.map(|col| Position {
        x: inner.x.saturating_add(col.min(inner.width)),
        y: inner.y,
    })
}

/// Centered Highlight modal: draft row only (history candidates float below).
/// Returns hardware cursor position for the draft caret.
pub fn render_highlight_modal(
    search: &HighlightBox,
    frame: &mut Frame,
    area: Rect,
) -> Option<Position> {
    let inner = render_modal_shell("Highlight", frame, area);
    let (spans, caret_col) = editable_text_spans(
        search.draft.as_str(),
        search.draft.cursor(),
        Some(inner.width),
    );
    frame.render_widget(Paragraph::new(Line::from(spans)), inner);
    Some(Position {
        x: inner.x.saturating_add(caret_col.min(inner.width)),
        y: inner.y,
    })
}
