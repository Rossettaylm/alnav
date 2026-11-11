//! Global time-window panel (`tt`).

use crate::app::App;
use crate::theme;
use crate::ui::{editable_text_spans, render_modal_shell};
use ratatui::layout::{Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;

/// Render `tt` time panel. Returns hardware cursor for the focused field.
pub fn render_time_panel(app: &App, frame: &mut Frame, area: Rect) -> Option<Position> {
    use crate::time_panel::TimeField;

    let panel = app.time_panel.as_ref()?;
    if area.height == 0 {
        return None;
    }
    let inner = render_modal_shell("Time", frame, area);
    if inner.height == 0 || inner.width == 0 {
        return None;
    }

    let focus = panel.focus;
    let cand_budget = inner
        .height
        .saturating_sub(6) // 2 labels + 4 field rows
        .min(5) as usize;

    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut caret_row: u16 = 0;
    let mut caret_col: u16 = 0;
    let mut row: u16 = 0;

    let push_label = |lines: &mut Vec<Line<'static>>, text: &str, row: &mut u16| {
        lines.push(Line::from(Span::styled(
            text.to_string(),
            Style::default().add_modifier(Modifier::DIM),
        )));
        *row = row.saturating_add(1);
    };

    let push_field = |lines: &mut Vec<Line<'static>>,
                      label: &str,
                      value: &str,
                      caret: usize,
                      active: bool,
                      row: &mut u16,
                      caret_row: &mut u16,
                      caret_col: &mut u16,
                      inner_w: u16| {
        let prefix = format!("{label} ");
        let prefix_w = UnicodeWidthStr::width(prefix.as_str()) as u16;
        let style = if active {
            Style::default().fg(theme::accent())
        } else {
            Style::default().add_modifier(Modifier::DIM)
        };
        let budget = inner_w.saturating_sub(prefix_w).max(1);
        let (value_spans, col) = editable_text_spans(value, caret, Some(budget));
        let mut spans = vec![Span::styled(prefix, style)];
        spans.extend(value_spans);
        lines.push(Line::from(spans));
        if active {
            *caret_row = *row;
            *caret_col = prefix_w.saturating_add(col);
        }
        *row = row.saturating_add(1);
    };

    push_label(&mut lines, "since", &mut row);
    push_field(
        &mut lines,
        "date",
        panel.since_date_query(),
        panel.since_date_cursor(),
        focus == TimeField::SinceDate,
        &mut row,
        &mut caret_row,
        &mut caret_col,
        inner.width,
    );
    if focus == TimeField::SinceDate {
        let filtered = panel.filtered_dates(true);
        let hl = panel.since_date_highlight();
        for (i, stats) in filtered.into_iter().take(cand_budget).enumerate() {
            let selected = hl == Some(i);
            let marker = if selected { ">" } else { " " };
            let style = if selected {
                theme::candidate_selected_style()
            } else {
                theme::candidate_unselected_style()
            };
            lines.push(Line::from(Span::styled(
                format!("{marker} {}", stats.date),
                style,
            )));
            row = row.saturating_add(1);
        }
    }
    push_field(
        &mut lines,
        "time",
        panel.since_time(),
        panel.since_time_cursor(),
        focus == TimeField::SinceTime,
        &mut row,
        &mut caret_row,
        &mut caret_col,
        inner.width,
    );

    push_label(&mut lines, "until", &mut row);
    push_field(
        &mut lines,
        "date",
        panel.until_date_query(),
        panel.until_date_cursor(),
        focus == TimeField::UntilDate,
        &mut row,
        &mut caret_row,
        &mut caret_col,
        inner.width,
    );
    if focus == TimeField::UntilDate {
        let filtered = panel.filtered_dates(false);
        let hl = panel.until_date_highlight();
        for (i, stats) in filtered.into_iter().take(cand_budget).enumerate() {
            let selected = hl == Some(i);
            let marker = if selected { ">" } else { " " };
            let style = if selected {
                theme::candidate_selected_style()
            } else {
                theme::candidate_unselected_style()
            };
            lines.push(Line::from(Span::styled(
                format!("{marker} {}", stats.date),
                style,
            )));
            row = row.saturating_add(1);
        }
    }
    push_field(
        &mut lines,
        "time",
        panel.until_time(),
        panel.until_time_cursor(),
        focus == TimeField::UntilTime,
        &mut row,
        &mut caret_row,
        &mut caret_col,
        inner.width,
    );

    let max_rows = inner.height as usize;
    lines.truncate(max_rows);
    frame.render_widget(Paragraph::new(lines), inner);

    let y = inner
        .y
        .saturating_add(caret_row.min(inner.height.saturating_sub(1)));
    // `caret_col == width` means after the last visible char.
    let x = inner.x.saturating_add(caret_col.min(inner.width));
    Some(Position { x, y })
}
