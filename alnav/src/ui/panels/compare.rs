//! Bookmark compare tray.

use crate::app::App;
use crate::theme;
use crate::ui::entry::render_compare_entry_lines;
use crate::ui::render_modal_shell_glyph;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, ListState};
use ratatui::Frame;

/// Bookmark compare list: log paint + Δt / stale prefixes. Always wraps.
pub fn render_compare_panel(app: &mut App, frame: &mut Frame, area: Rect) {
    let Some(_) = app.compare.as_ref() else {
        return;
    };
    let inner = render_modal_shell_glyph(theme::GLYPH_BOOKMARK, "Bookmark Compare", frame, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }
    let sorted = app.bookmarks.sorted_indices();
    let deltas = app.bookmarks.delta_labels();
    let delta_col = deltas
        .iter()
        .map(|d| {
            d.as_deref()
                .unwrap_or("")
                .chars()
                .count()
                .max(theme::GLYPH_COMPARE_UNTIMED.chars().count())
        })
        .max()
        .unwrap_or(1);
    let prefix_w = 1 + 1 + delta_col + 1; // mark space delta space
    let entry_w = (inner.width as usize).saturating_sub(prefix_w).max(8);
    let patterns = app.highlight_groups.paint_patterns(app.active_highlight);
    let cursor = app.compare.as_ref().map(|p| p.cursor).unwrap_or(0);

    let mut items: Vec<ListItem> = Vec::with_capacity(sorted.len());
    for (display_i, &storage) in sorted.iter().enumerate() {
        let row = app.bookmarks.items[storage].row.clone();
        let row_id = row.row_id;
        let stale = !app.bookmark_alive(row_id);
        let mark = if stale {
            theme::GLYPH_BOOKMARK_STALE_MARK
        } else {
            " "
        };
        let delta_text = deltas
            .get(display_i)
            .and_then(|d| d.as_deref())
            .unwrap_or("");
        let delta_pad = format!("{delta_text:>delta_col$}");
        let mut lines = render_compare_entry_lines(&row, &patterns, entry_w);
        if let Some(first) = lines.first_mut() {
            let mut prefix = vec![
                Span::styled(
                    format!("{mark} "),
                    if stale {
                        theme::bookmark_stale_style()
                    } else {
                        Style::default()
                    },
                ),
                Span::styled(format!("{delta_pad} "), theme::compare_delta_style()),
            ];
            prefix.append(&mut first.spans);
            *first = Line::from(prefix);
        }
        let mut item = ListItem::new(lines);
        if display_i == cursor {
            item = item.style(theme::log_selection_style());
        }
        items.push(item);
    }

    let list = List::new(items);
    let mut state =
        ListState::default().with_offset(app.compare.as_ref().map(|p| p.list_offset).unwrap_or(0));
    if !sorted.is_empty() {
        state.select(Some(cursor.min(sorted.len() - 1)));
    }
    frame.render_stateful_widget(list, inner, &mut state);
    if let Some(panel) = app.compare.as_mut() {
        panel.list_offset = state.offset();
    }
}
