//! Histogram panel: bucket bars and preview rows.

use crate::app::App;
use crate::theme;
use crate::ui::render_modal_shell;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

const HIST_PREVIEW_ROWS: usize = 6;

const HIST_BAR_WIDTH: usize = 16;

pub fn hist_content_row_count(app: &App) -> usize {
    use crate::hist_panel::HistView;
    match &app.hist_view {
        HistView::Closed => 0,
        HistView::Loading { .. } => 1,
        HistView::Ready(report) => report.buckets.len().saturating_add(HIST_PREVIEW_ROWS + 2),
    }
    .max(1)
}

fn hist_bucket_line(
    bucket: &crate::hist_panel::HistBucket,
    selected: bool,
    spike: bool,
) -> Line<'static> {
    use alnav::parser::Level;
    let total = bucket.total().max(1);
    let width = HIST_BAR_WIDTH;
    let segs = [
        (bucket.v, Level::V),
        (bucket.d, Level::D),
        (bucket.i, Level::I),
        (bucket.w, Level::W),
        (bucket.e, Level::E),
        (bucket.f, Level::F),
    ];
    let mut filled = 0usize;
    let mut spans = Vec::new();
    let label_style = if selected {
        theme::candidate_selected_style()
    } else {
        Style::default().add_modifier(Modifier::DIM)
    };
    let mark = if selected { "› " } else { "  " };
    spans.push(Span::styled(format!("{mark}{} ", bucket.key), label_style));
    for (count, level) in segs {
        let n = ((count as f64 / total as f64) * width as f64).round() as usize;
        let n = n.min(width.saturating_sub(filled));
        if n > 0 {
            spans.push(Span::styled("█".repeat(n), theme::level_bar_style(level)));
            filled += n;
        }
    }
    if filled < width {
        spans.push(Span::styled(
            "░".repeat(width - filled),
            Style::default().add_modifier(Modifier::DIM),
        ));
    }
    let mut tail = format!(" {}", bucket.total());
    if bucket.severe > 0 {
        tail.push_str(&format!(" E{}", bucket.severe));
    }
    if spike {
        tail.push_str(" !");
    }
    spans.push(Span::styled(
        tail,
        if selected {
            theme::candidate_selected_style()
        } else if bucket.severe > 0 {
            theme::severe_entry_style(false)
        } else {
            Style::default().add_modifier(Modifier::DIM)
        },
    ));
    Line::from(spans)
}

pub fn render_hist_panel(app: &App, frame: &mut Frame, area: Rect) {
    use crate::hist_panel::HistView;
    if area.height == 0 {
        return;
    }
    let title = match &app.hist_view {
        HistView::Ready(r) => {
            format!(
                "Hist {}",
                alnav::histogram::format_interval(r.interval_secs)
            )
        }
        HistView::Loading { interval_secs } => {
            format!(
                "Hist {} ",
                alnav::histogram::format_interval(*interval_secs)
            )
        }
        HistView::Closed => "Hist".into(),
    };
    let inner = render_modal_shell(&title, frame, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }
    match &app.hist_view {
        HistView::Closed => {}
        HistView::Loading { .. } => {
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    "computing…",
                    theme::log_loading_style(true),
                ))),
                inner,
            );
        }
        HistView::Ready(report) => {
            let preview_h = HIST_PREVIEW_ROWS.min(inner.height.saturating_sub(3) as usize);
            let chunks =
                Layout::vertical([Constraint::Min(1), Constraint::Length(preview_h as u16)])
                    .split(inner);
            let list_h = chunks[0].height as usize;
            let start = app
                .hist_cursor
                .saturating_sub(list_h.saturating_sub(1) / 2)
                .min(report.buckets.len().saturating_sub(list_h));
            let lines: Vec<Line<'static>> = report
                .buckets
                .iter()
                .enumerate()
                .skip(start)
                .take(list_h)
                .map(|(i, b)| hist_bucket_line(b, i == app.hist_cursor, report.is_spike(&b.key)))
                .collect();
            frame.render_widget(Paragraph::new(lines), chunks[0]);
            if let Some(bucket) = report.buckets.get(app.hist_cursor) {
                let mut preview = Vec::new();
                preview.push(Line::from(Span::styled(
                    "preview",
                    Style::default().add_modifier(Modifier::DIM),
                )));
                for row in bucket.preview.iter().take(preview_h.saturating_sub(1)) {
                    preview.push(Line::from(Span::styled(
                        format!("{} {} {}", row.timestamp, row.tag, row.msg),
                        theme::muted(),
                    )));
                }
                if preview.len() == 1 {
                    preview.push(Line::from(Span::styled(
                        "no rows",
                        theme::preview_placeholder_style(),
                    )));
                }
                frame.render_widget(Paragraph::new(preview), chunks[1]);
            }
        }
    }
}
