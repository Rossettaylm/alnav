//! Bottom status bar.

use super::strips::span_width;
use crate::app::App;
use crate::theme;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

const FLASH_MIN: usize = 12;

pub fn render_status_bar(app: &mut App, frame: &mut Frame, area: Rect) {
    let mut left = vec![Span::styled(
        format!("{}/{}", app.cursor + 1, app.index.visible.len()),
        Style::default().add_modifier(Modifier::DIM),
    )];
    if let Some((current, total)) = app.highlight_match_stats() {
        let k = current.map(|n| n.to_string()).unwrap_or_else(|| "-".into());
        left.push(Span::raw(" "));
        left.push(theme::status_pill_value(
            theme::GLYPH_SEARCH,
            &format!("{k}/{total}"),
            theme::accent(),
        ));
    }
    left.push(Span::raw(" "));
    if app.following {
        left.push(theme::status_pill(theme::GLYPH_FOLLOWING, theme::success()));
    } else {
        left.push(theme::status_icon_dim(theme::GLYPH_FOLLOWING));
    }
    // Source vs disconnect share one slot (live disconnect replaces source glyph).
    left.push(Span::raw(" "));
    if app.export_source.is_live() && app.ingest_done {
        left.push(theme::status_pill(
            theme::GLYPH_DISCONNECT,
            theme::warning(),
        ));
    } else {
        left.push(theme::status_pill(
            app.export_source.status_glyph(),
            theme::accent(),
        ));
    }
    if let Some(lock) = app.lock_badge_label() {
        left.push(Span::raw(" "));
        left.push(theme::status_pill_value(
            theme::GLYPH_LOCK,
            &lock,
            theme::lock(),
        ));
    }
    if let Some(time) = app.time_badge_label() {
        left.push(Span::raw(" "));
        left.push(theme::status_pill_value(
            theme::GLYPH_TIME,
            &time,
            theme::lock(),
        ));
    }
    if let Some(focus) = app.view_focus_badge_label() {
        left.push(Span::raw(" "));
        left.push(theme::status_pill_value(
            theme::GLYPH_VIEW_FOCUS,
            focus,
            theme::accent(),
        ));
    }
    if let Some(prog) = app.file_progress_label() {
        left.push(Span::raw(" "));
        left.push(theme::status_pill_value(
            theme::GLYPH_PROGRESS,
            &prog,
            theme::warning(),
        ));
    }
    if app.visual_anchor.is_some() {
        left.push(Span::raw(" "));
        left.push(theme::status_pill(theme::GLYPH_VISUAL, theme::accent()));
    }

    let area_w = area.width as usize;
    let left_w: usize = left.iter().map(span_width).sum();
    let avail = area_w.saturating_sub(left_w);

    let mut flash_span: Option<Span<'static>> = None;
    let mut flash_block_w = 0usize;
    if let Some(msg) = app.status_msg.as_deref() {
        let full = theme::status_flash_pill(msg);
        let natural = 1 + span_width(&full);
        let slot = natural.max(FLASH_MIN);
        let hint_budget = avail.saturating_sub(slot).saturating_sub(1);
        if hint_budget < crate::help::MIN_HELP_WIDTH && natural > avail {
            let pill_max = avail.saturating_sub(1);
            if pill_max == 0 {
                flash_span = None;
                flash_block_w = 0;
            } else {
                let fitted = theme::status_flash_pill_fit(msg, pill_max);
                flash_block_w = 1 + span_width(&fitted);
                flash_span = Some(fitted);
            }
        } else {
            flash_span = Some(full);
            flash_block_w = natural;
        }
    }

    let hint_budget = if flash_span.is_some() {
        let slot = flash_block_w.max(FLASH_MIN);
        avail.saturating_sub(slot).saturating_sub(1)
    } else {
        avail.saturating_sub(1)
    };
    let hints = crate::help::context_hint_spans(app, hint_budget);
    let hint_w: usize = hints
        .as_ref()
        .map(|h| h.iter().map(span_width).sum())
        .unwrap_or(0);

    let mut spans = left;
    if let Some(flash) = flash_span {
        spans.push(Span::raw(" "));
        spans.push(flash);
    }
    if hints.is_some() {
        let pad = area_w.saturating_sub(left_w + flash_block_w + hint_w);
        if pad > 0 {
            spans.push(Span::raw(" ".repeat(pad)));
        }
        if let Some(hint) = hints {
            spans.extend(hint);
        }
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}
