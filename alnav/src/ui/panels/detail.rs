//! Detail modal: field lines, pretty JSON and crash detail context.

use crate::app::App;
use crate::model::EntryRow;
use crate::theme;
use crate::ui::entry::wrap_ranges;
use crate::ui::render_modal_shell;
use alnav::crash::{CrashDetector, CrashInfo, CrashType};
use alnav::parser::LogEntry;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use std::sync::OnceLock;

/// Build H4 Fields-mode lines for the current row (used by render + height).
pub fn detail_field_lines(
    row: Option<&crate::model::EntryRow>,
    inner_width: u16,
) -> Vec<Line<'static>> {
    use crate::input::ChipField;

    let label_w = 5usize;
    let value_w = inner_width.saturating_sub(label_w as u16 + 1).max(1) as usize;

    let mut lines: Vec<Line<'static>> = Vec::new();
    let Some(row) = row else {
        lines.push(Line::from(Span::styled(
            "no row".to_string(),
            theme::preview_placeholder_style(),
        )));
        return lines;
    };

    let push_kv = |lines: &mut Vec<Line<'static>>,
                   label: &str,
                   label_style: Style,
                   value: String,
                   value_style: Style| {
        let label_pad = format!("{label:<width$}", width = label_w);
        let mut first = true;
        for (s, e) in wrap_ranges(&value, value_w) {
            let chunk = value[s..e].to_string();
            if first {
                lines.push(Line::from(vec![
                    Span::styled(label_pad.clone(), label_style),
                    Span::raw(" "),
                    Span::styled(chunk, value_style),
                ]));
                first = false;
            } else {
                lines.push(Line::from(vec![
                    Span::raw(" ".repeat(label_w + 1)),
                    Span::styled(chunk, value_style),
                ]));
            }
        }
        if first {
            lines.push(Line::from(vec![
                Span::styled(label_pad, label_style),
                Span::raw(" "),
            ]));
        }
    };

    push_kv(
        &mut lines,
        "time",
        theme::detail_label_style(),
        row.timestamp.clone(),
        theme::muted(),
    );
    {
        let level_ch = row.level.as_char().to_string();
        let label_pad = format!("{:<width$}", "level", width = label_w);
        lines.push(Line::from(vec![
            Span::styled(label_pad, theme::detail_field_label_style(ChipField::Level)),
            Span::raw(" "),
            Span::styled(format!(" {level_ch} "), theme::level_badge_style(row.level)),
        ]));
    }
    push_kv(
        &mut lines,
        ChipField::Pid.keyword(),
        theme::detail_field_label_style(ChipField::Pid),
        row.pid.clone(),
        Style::default(),
    );
    push_kv(
        &mut lines,
        ChipField::Tid.keyword(),
        theme::detail_field_label_style(ChipField::Tid),
        row.tid.clone(),
        Style::default(),
    );
    push_kv(
        &mut lines,
        ChipField::Tag.keyword(),
        theme::detail_field_label_style(ChipField::Tag),
        row.tag.clone(),
        Style::default(),
    );
    push_kv(
        &mut lines,
        ChipField::Pkg.keyword(),
        theme::detail_field_label_style(ChipField::Pkg),
        row.pkg.clone(),
        Style::default(),
    );
    push_kv(
        &mut lines,
        ChipField::Msg.keyword(),
        theme::detail_field_label_style(ChipField::Msg),
        row.msg.clone(),
        Style::default(),
    );
    lines
}

/// Try JSON pretty-print: `msg` first, then `raw`. Returns `(text, is_json)`.
pub fn pretty_json_for_row(row: &crate::model::EntryRow) -> (String, bool) {
    for candidate in [&row.msg, &row.raw] {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(candidate.trim()) {
            if let Ok(pretty) = serde_json::to_string_pretty(&value) {
                return (pretty, true);
            }
        }
    }
    (row.msg.clone(), false)
}

/// H5 Pretty-mode lines (used by render + height).
pub fn detail_pretty_lines(
    row: Option<&crate::model::EntryRow>,
    inner_width: u16,
) -> Vec<Line<'static>> {
    let width = inner_width.max(1) as usize;
    let mut lines: Vec<Line<'static>> = Vec::new();
    let Some(row) = row else {
        lines.push(Line::from(Span::styled(
            "no row".to_string(),
            theme::preview_placeholder_style(),
        )));
        return lines;
    };
    let (text, is_json) = pretty_json_for_row(row);
    if !is_json {
        lines.push(Line::from(Span::styled(
            "not JSON".to_string(),
            theme::preview_placeholder_style(),
        )));
    }
    for (s, e) in wrap_ranges(&text, width) {
        lines.push(Line::from(Span::raw(text[s..e].to_string())));
    }
    lines
}

fn crash_detector() -> &'static CrashDetector {
    static DETECTOR: OnceLock<CrashDetector> = OnceLock::new();
    DETECTOR.get_or_init(CrashDetector::new)
}

/// Continuation-line scan cap for File-mode crash stack merging (R2).
const CRASH_SCAN_LIMIT: usize = 500;

/// Structured crash/ANR info for the cursor row's msg, when it matches a
/// crash signature. `None` lets the caller fall back to the existing
/// JSON/raw Pretty chain (R1). The `bool` flags a File-mode continuation
/// scan that hit [`CRASH_SCAN_LIMIT`] before finding the next parsed row.
pub fn crash_context_for_row(app: &App, row: &EntryRow) -> Option<(CrashInfo, bool)> {
    let crash_type = crash_detector().detect(&row.as_log_entry())?;

    let (merged_msg, truncated) = if app.index.store.is_file() {
        let mut merged = row.msg.clone();
        let mut truncated = false;
        if let Some(start) = app.source_idx_for_visible(app.cursor) {
            let mut idx = start + 1;
            let mut scanned = 0usize;
            loop {
                if scanned >= CRASH_SCAN_LIMIT {
                    truncated = true;
                    break;
                }
                let Some(next) = app.index.store.row_at_source(idx, false) else {
                    break;
                };
                if next.parsed {
                    break;
                }
                merged.push('\n');
                merged.push_str(&next.msg);
                idx += 1;
                scanned += 1;
            }
        }
        (merged, truncated)
    } else {
        (row.msg.clone(), false)
    };

    let entry = LogEntry {
        timestamp: &row.timestamp,
        pid: &row.pid,
        tid: &row.tid,
        level: row.level,
        tag: &row.tag,
        pkg: &row.pkg,
        msg: &merged_msg,
    };
    Some((crash_detector().parse_crash(&entry, crash_type), truncated))
}

/// H_crash structured detail lines (R4). `is_stream` picks the empty-stack
/// placeholder copy; `truncated` appends the 500-line scan-cap notice.
pub fn render_crash_detail_lines(
    info: &CrashInfo,
    is_stream: bool,
    truncated: bool,
    width: usize,
) -> Vec<Line<'static>> {
    let width = width.max(1);
    let mut lines: Vec<Line<'static>> = Vec::new();

    let type_label = match info.crash_type {
        CrashType::FatalException => "FATAL EXCEPTION",
        CrashType::Anr => "ANR",
        CrashType::NativeCrash => "NATIVE CRASH",
    };
    let badge_style = Style::default()
        .fg(theme::warning())
        .add_modifier(Modifier::BOLD);
    lines.push(Line::from(vec![
        Span::styled(format!("{} ", theme::GLYPH_CRASH), badge_style),
        Span::styled(type_label.to_string(), badge_style),
    ]));
    for (s, e) in wrap_ranges(&info.headline, width) {
        lines.push(Line::from(Span::raw(info.headline[s..e].to_string())));
    }
    if let Some(exception) = &info.exception {
        for (s, e) in wrap_ranges(exception, width) {
            lines.push(Line::from(Span::styled(
                exception[s..e].to_string(),
                Style::default().fg(theme::warning()),
            )));
        }
    }
    lines.push(Line::from(Span::styled(
        format!(
            "pid={} tid={} tag={} time={}",
            info.pid, info.tid, info.tag, info.timestamp
        ),
        theme::muted(),
    )));
    if info.stack.is_empty() {
        let placeholder = if is_stream {
            "no stack (stream)"
        } else {
            "no stack"
        };
        lines.push(Line::from(Span::styled(
            placeholder.to_string(),
            theme::preview_placeholder_style(),
        )));
    } else {
        for frame in &info.stack {
            for (s, e) in wrap_ranges(frame, width) {
                lines.push(Line::from(Span::raw(frame[s..e].to_string())));
            }
        }
    }
    if truncated {
        lines.push(Line::from(Span::styled(
            "…(truncated)".to_string(),
            theme::preview_placeholder_style(),
        )));
    }
    lines
}

/// Content lines for the current detail mode (height estimation + render).
pub fn detail_content_lines(app: &App, inner_width: u16) -> Vec<Line<'static>> {
    use crate::app::DetailView;
    match app.detail {
        DetailView::Fields => detail_field_lines(app.current_row().as_deref(), inner_width),
        DetailView::Pretty => {
            if let Some(row) = app.current_row() {
                if let Some((info, truncated)) = crash_context_for_row(app, &row) {
                    let width = inner_width.max(1) as usize;
                    return render_crash_detail_lines(
                        &info,
                        !app.index.store.is_file(),
                        truncated,
                        width,
                    );
                }
            }
            detail_pretty_lines(app.current_row().as_deref(), inner_width)
        }
        DetailView::Closed => Vec::new(),
    }
}

/// H4/H5 row-detail overlay.
pub fn render_detail(app: &App, frame: &mut Frame, area: Rect) {
    use crate::app::DetailView;
    if matches!(app.detail, DetailView::Closed) || area.height == 0 {
        return;
    }
    let title = match app.detail {
        DetailView::Fields => "Detail",
        DetailView::Pretty => "Pretty",
        DetailView::Closed => return,
    };
    let inner = render_modal_shell(title, frame, area);
    let lines = detail_content_lines(app, inner.width);
    let max_rows = inner.height as usize;
    let shown: Vec<Line<'static>> = lines.into_iter().take(max_rows).collect();
    frame.render_widget(Paragraph::new(shown), inner);
}
