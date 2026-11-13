//! Highlight modal renderer (draft row with hardware cursor).

use crate::highlight_model::HighlightBox;
use crate::ui::{editable_text_spans, render_modal_shell};
use ratatui::layout::{Position, Rect};
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use ratatui::Frame;

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
