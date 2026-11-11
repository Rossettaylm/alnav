//! Summary panel: level / tag bars and report lines.

use crate::app::App;
use crate::theme;
use crate::ui::render_modal_shell;
use crate::ui::text::pad_display;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

/// Horizontal Unicode-block bar (`█`/`░`), proportional to `count / max`.
/// Shared by the summary panel's level-distribution and Top-tags sections
/// (Top errors intentionally has no bar — see `render_summary_panel`).
pub(in crate::ui) fn bar_line(
    label: &str,
    count: usize,
    max: usize,
    width: usize,
    color: Style,
) -> Line<'static> {
    let ratio = if max == 0 {
        0.0
    } else {
        count as f64 / max as f64
    };
    let filled = ((ratio * width as f64).round() as usize).min(width);
    let empty = width.saturating_sub(filled);
    Line::from(vec![
        Span::styled(
            pad_display(label, SUMMARY_LABEL_WIDTH),
            Style::default().add_modifier(Modifier::DIM),
        ),
        Span::raw(" "),
        Span::styled("█".repeat(filled), color),
        Span::styled(
            "░".repeat(empty),
            Style::default().add_modifier(Modifier::DIM),
        ),
        Span::raw(format!(" {count}")),
    ])
}

const SUMMARY_BAR_WIDTH: usize = 20;

const SUMMARY_LABEL_WIDTH: usize = 12;

/// Body lines for the Ready summary panel (used by render + height calc).
/// Aligned with CLI `--summary` fields: total rows, time range, crashes,
/// level distribution (bar), Top 10 tags (bar), Top 10 errors (no bar).
pub(in crate::ui) fn summary_report_lines(
    report: &alnav::summary::SummaryOutput,
) -> Vec<Line<'static>> {
    use alnav::parser::Level;

    let mut lines: Vec<Line<'static>> = Vec::new();
    lines.push(Line::from(Span::raw(format!("Rows: {}", report.total))));
    let first = if report.time_range.first.is_empty() {
        "-"
    } else {
        report.time_range.first.as_str()
    };
    let last = if report.time_range.last.is_empty() {
        "-"
    } else {
        report.time_range.last.as_str()
    };
    lines.push(Line::from(Span::raw(format!(
        "Time range: {first} — {last}"
    ))));
    lines.push(Line::from(Span::raw(format!(
        "Crashes: {}",
        report.crashes
    ))));
    lines.push(Line::default());

    lines.push(Line::from(Span::styled(
        "Levels",
        Style::default().add_modifier(Modifier::BOLD),
    )));
    let level_order = [Level::V, Level::D, Level::I, Level::W, Level::E, Level::F];
    let level_max = level_order
        .iter()
        .map(|l| *report.levels.get(&l.as_char()).unwrap_or(&0))
        .max()
        .unwrap_or(0);
    for level in level_order {
        let count = *report.levels.get(&level.as_char()).unwrap_or(&0);
        if count == 0 {
            continue;
        }
        lines.push(bar_line(
            &level.as_char().to_string(),
            count,
            level_max,
            SUMMARY_BAR_WIDTH,
            theme::level_bar_style(level),
        ));
    }
    lines.push(Line::default());

    lines.push(Line::from(Span::styled(
        "Top tags",
        Style::default().add_modifier(Modifier::BOLD),
    )));
    if report.top_tags.is_empty() {
        lines.push(Line::from(Span::styled(
            "(none)",
            theme::preview_placeholder_style(),
        )));
    }
    let tag_max = report.top_tags.iter().map(|t| t.count).max().unwrap_or(0);
    for entry in &report.top_tags {
        lines.push(bar_line(
            &entry.tag,
            entry.count,
            tag_max,
            SUMMARY_BAR_WIDTH,
            theme::accent_bar_style(),
        ));
        let mut parts: Vec<String> = entry
            .levels
            .iter()
            .map(|(c, n)| format!("{c}:{n}"))
            .collect();
        parts.sort();
        if !parts.is_empty() {
            lines.push(Line::from(Span::styled(
                format!("    {}", parts.join(" ")),
                Style::default().add_modifier(Modifier::DIM),
            )));
        }
    }
    lines.push(Line::default());

    lines.push(Line::from(Span::styled(
        "Top errors",
        Style::default().add_modifier(Modifier::BOLD),
    )));
    if report.top_errors.is_empty() {
        lines.push(Line::from(Span::styled(
            "(none)",
            theme::preview_placeholder_style(),
        )));
    }
    for (i, err) in report.top_errors.iter().enumerate() {
        lines.push(Line::from(Span::raw(format!(
            "{:>2}. [{}] {} ({})",
            i + 1,
            err.tag,
            err.pattern,
            err.count
        ))));
        lines.push(Line::from(Span::styled(
            format!("    {}", err.sample),
            Style::default().add_modifier(Modifier::DIM),
        )));
    }
    lines
}

/// Row count for `Loading` (1, the placeholder line) or `Ready` content —
/// used by `main.rs` to size the modal before `render_summary_panel` runs.
pub fn summary_content_row_count(app: &App) -> usize {
    use crate::app::SummaryView;
    match &app.summary_view {
        SummaryView::Closed => 0,
        SummaryView::Loading => 1,
        SummaryView::Ready(report) => summary_report_lines(report).len(),
    }
    .max(1)
}

/// Leader `i` summary panel: `Loading` placeholder or `Ready` stats body.
/// `Esc` closes without resuming follow (`app.close_summary_panel`); content
/// is a static snapshot — it never refreshes while open (see PRD R1).
pub fn render_summary_panel(app: &App, frame: &mut Frame, area: Rect) {
    use crate::app::SummaryView;
    if area.height == 0 {
        return;
    }
    let inner = render_modal_shell("Summary", frame, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }
    match &app.summary_view {
        SummaryView::Closed => {}
        SummaryView::Loading => {
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    "computing…",
                    theme::log_loading_style(true),
                ))),
                inner,
            );
        }
        SummaryView::Ready(report) => {
            let lines = summary_report_lines(report);
            let scroll = app.summary_scroll.min(lines.len().saturating_sub(1));
            let visible: Vec<Line<'static>> = lines.into_iter().skip(scroll).collect();
            frame.render_widget(
                Paragraph::new(visible).wrap(ratatui::widgets::Wrap { trim: false }),
                inner,
            );
        }
    }
}
