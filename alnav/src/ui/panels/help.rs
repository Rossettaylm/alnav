//! Help panel and its search prompt.

use crate::app::App;
use crate::theme;
use crate::ui::{editable_text_spans, render_modal_shell};
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;

/// Read-only Help panel (`?`): Home (Active + TOC) or a zone page.
pub fn render_help_panel(app: &mut App, frame: &mut Frame, area: Rect) -> Option<Position> {
    let title = format!("{} Help", theme::GLYPH_HELP);
    let inner = render_modal_shell(&title, frame, area);
    if inner.width == 0 || inner.height == 0 {
        return None;
    }

    let prompting = app.help_search.as_ref().is_some_and(|s| s.prompt);
    let chrome = crate::help::chrome_help_line();
    let chrome_idx = crate::help::home_active_len(app) + crate::help::HelpPage::ALL.len();
    let chrome_hits = crate::help::hits_on_line(app.help_search.as_ref(), None, chrome_idx);
    let chrome_line = crate::help::overlay_search_hits(chrome.line, &chrome_hits);

    let prompt_h = u16::from(prompting);
    let chrome_h = u16::from(!prompting);
    let reserved = prompt_h.saturating_add(chrome_h);
    let mut cursor = None;

    match app.help_view {
        crate::help::HelpView::Home { toc, toc_off } => {
            let active = crate::help::home_active_lines(app);
            let active_len = active.len();
            // Pin Active + chrome, but keep at least one TOC row when the
            // frame is short enough that a full Active block would hide it.
            let budget = inner.height.saturating_sub(reserved);
            let min_active = 1u16;
            let max_active = if budget >= (active_len as u16).saturating_add(1) {
                active_len as u16
            } else if budget > min_active {
                budget.saturating_sub(1).max(min_active)
            } else {
                budget.min(active_len as u16)
            };
            let toc_h = inner
                .height
                .saturating_sub(max_active.saturating_add(reserved));
            let view_h = toc_h as usize;
            let sel = toc as usize;
            let mut off = toc_off;
            if view_h > 0 {
                if sel < off {
                    off = sel;
                } else if sel >= off.saturating_add(view_h) {
                    off = sel + 1 - view_h;
                }
            }
            if let crate::help::HelpView::Home { toc_off, .. } = &mut app.help_view {
                *toc_off = off;
            }

            let chunks = Layout::vertical([
                Constraint::Length(max_active),
                Constraint::Min(0),
                Constraint::Length(prompt_h),
                Constraint::Length(chrome_h),
            ])
            .split(inner);

            let active_painted: Vec<Line<'static>> = active
                .into_iter()
                .enumerate()
                .map(|(i, row)| {
                    let hits = crate::help::hits_on_line(app.help_search.as_ref(), None, i);
                    crate::help::overlay_search_hits(row.line, &hits)
                })
                .collect();
            frame.render_widget(
                Paragraph::new(active_painted).wrap(ratatui::widgets::Wrap { trim: false }),
                chunks[0],
            );

            let toc_rows = crate::help::home_toc_lines(Some(toc));
            let visible: Vec<Line<'static>> = toc_rows
                .into_iter()
                .enumerate()
                .skip(off)
                .take(view_h)
                .map(|(i, row)| {
                    let hits =
                        crate::help::hits_on_line(app.help_search.as_ref(), None, active_len + i);
                    crate::help::overlay_search_hits(row.line, &hits)
                })
                .collect();
            frame.render_widget(Paragraph::new(visible), chunks[1]);

            if prompting {
                cursor = render_help_prompt(app, frame, chunks[2]);
            } else if chrome_h > 0 {
                frame.render_widget(Paragraph::new(chrome_line), chunks[3]);
            }
        }
        crate::help::HelpView::Page { id, scroll } => {
            let body = crate::help::page_doc_lines(app, id);
            let n = body.len();
            let chunks = Layout::vertical([
                Constraint::Min(1),
                Constraint::Length(prompt_h),
                Constraint::Length(chrome_h),
            ])
            .split(inner);
            let view_h = chunks[0].height as usize;
            app.help_body_view_h = view_h.max(1);
            let max_scroll = crate::help::page_max_scroll(n, app.help_body_view_h);
            let scroll = scroll.min(max_scroll);
            if let crate::help::HelpView::Page { scroll: stored, .. } = &mut app.help_view {
                *stored = scroll;
            }
            let painted: Vec<Line<'static>> = body
                .into_iter()
                .enumerate()
                .skip(scroll)
                .map(|(i, row)| {
                    let hits = crate::help::hits_on_line(app.help_search.as_ref(), Some(id), i);
                    crate::help::overlay_search_hits(row.line, &hits)
                })
                .collect();
            frame.render_widget(
                Paragraph::new(painted).wrap(ratatui::widgets::Wrap { trim: false }),
                chunks[0],
            );
            if prompting {
                cursor = render_help_prompt(app, frame, chunks[1]);
            } else if chrome_h > 0 {
                frame.render_widget(Paragraph::new(chrome_line), chunks[2]);
            }
        }
    }
    cursor
}

fn render_help_prompt(app: &App, frame: &mut Frame, area: Rect) -> Option<Position> {
    let Some(search) = app.help_search.as_ref() else {
        return None;
    };
    if area.height == 0 || area.width == 0 {
        return None;
    }
    let prefix = "/ ";
    let prefix_w = UnicodeWidthStr::width(prefix) as u16;
    let q_width = area.width.saturating_sub(prefix_w);
    let (q_spans, caret_col) =
        editable_text_spans(search.query.as_str(), search.query.cursor(), Some(q_width));
    let mut spans = vec![Span::styled(
        prefix.to_string(),
        theme::context_help_style(),
    )];
    spans.extend(q_spans);
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
    Some(Position {
        x: area
            .x
            .saturating_add(prefix_w.saturating_add(caret_col.min(q_width.saturating_sub(1)))),
        y: area.y,
    })
}
