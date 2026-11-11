//! Main log list with minimap rail and the bookmark summary strip.

use super::entry::{render_entry_line_collapsed, render_entry_lines_ex};
use super::rounded_block;
use crate::app::{App, Focus};
use crate::theme;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, List, ListItem, ListState, Paragraph};
use ratatui::Frame;

/// H3 minimap cell priority (higher wins on overlap).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum MinimapMark {
    Track = 0,
    Viewport = 1,
    Highlight = 2,
    Bookmark = 3,
    Severe = 4,
}

/// Max visible indices scanned per frame for search/severe marks (H3).
const MINIMAP_MARK_BUDGET: usize = 4000;

/// Map a `visible` index into a rail row (`height` cells).
pub fn minimap_row_for_index(index: usize, visible_len: usize, height: usize) -> usize {
    if height == 0 {
        return 0;
    }
    if visible_len <= 1 || height == 1 {
        return 0;
    }
    index.saturating_mul(height - 1) / (visible_len - 1)
}

/// Build H3 rail marks for `height` cells. Empty when `visible` is empty.
pub fn build_minimap_marks(app: &App, height: u16) -> Vec<MinimapMark> {
    let h = height as usize;
    if h == 0 || app.index.visible.is_empty() {
        return Vec::new();
    }
    let n = app.index.visible.len();
    let mut cells = vec![MinimapMark::Track; h];

    // Approximate viewport band from list_offset (item units ≈ 1 row each).
    let start = app.list_offset.min(n.saturating_sub(1));
    let vp_items = h.max(1).min(n);
    let end = (start + vp_items).min(n).max(start + 1);
    let v0 = minimap_row_for_index(start, n, h);
    let v1 = minimap_row_for_index(end - 1, n, h);
    for r in v0..=v1 {
        if cells[r] < MinimapMark::Viewport {
            cells[r] = MinimapMark::Viewport;
        }
    }

    // File: never O(budget) `row_at` — severe via prefetch cache, highlight via
    // async hit index. Stream keeps owned-row sample parse (bounded buffer).
    if app.index.store.is_file() {
        let samples = n.min(MINIMAP_MARK_BUDGET);
        for s in 0..samples {
            let i = if samples <= 1 {
                0
            } else {
                s * (n - 1) / (samples - 1)
            };
            let Some(src) = app.source_idx_for_visible(i) else {
                continue;
            };
            if app.index.store.as_file().and_then(|f| f.severe_cached(src)) == Some(true) {
                let r = minimap_row_for_index(i, n, h);
                cells[r] = MinimapMark::Severe;
            }
        }
        let hits = &app.highlight_scan.hits;
        let hit_n = hits.len();
        if hit_n > 0 {
            let take = hit_n.min(MINIMAP_MARK_BUDGET);
            for s in 0..take {
                let hi = if take <= 1 {
                    0
                } else {
                    s * (hit_n - 1) / (take - 1)
                };
                let i = hits[hi];
                if i >= n {
                    continue;
                }
                let r = minimap_row_for_index(i, n, h);
                if cells[r] < MinimapMark::Highlight {
                    cells[r] = MinimapMark::Highlight;
                }
            }
        }
    } else {
        let samples = n.min(MINIMAP_MARK_BUDGET);
        for s in 0..samples {
            let i = if samples <= 1 {
                0
            } else {
                s * (n - 1) / (samples - 1)
            };
            let Some(row) = app.row_at(i) else {
                continue;
            };
            let r = minimap_row_for_index(i, n, h);
            if app.highlight_groups.any_match(&row.tag, &row.msg)
                && cells[r] < MinimapMark::Highlight
            {
                cells[r] = MinimapMark::Highlight;
            }
            if row.severe {
                cells[r] = MinimapMark::Severe;
            }
        }
    }

    // Bookmarks (F5): O(bookmarks) via row_id→visible lookup — never scan /
    // parse all visible rows (FileStore would O(n) parse multi-million files).
    if !app.bookmarks.items.is_empty() {
        for bm in &app.bookmarks.items {
            if !app.bookmark_alive(bm.row_id()) {
                continue;
            }
            if let Some(i) = app.visible_idx_for_row_id(bm.row_id()) {
                let r = minimap_row_for_index(i, n, h);
                if cells[r] < MinimapMark::Bookmark {
                    cells[r] = MinimapMark::Bookmark;
                }
            }
        }
    }
    cells
}

fn render_minimap(app: &App, frame: &mut Frame, rail: Rect) {
    if rail.width == 0 || rail.height == 0 {
        return;
    }
    let marks = build_minimap_marks(app, rail.height);
    if marks.is_empty() {
        return;
    }
    let buf = frame.buffer_mut();
    for (dy, mark) in marks.iter().enumerate() {
        let y = rail.y.saturating_add(dy as u16);
        if y >= rail.y.saturating_add(rail.height) {
            break;
        }
        let cell = &mut buf[(rail.x, y)];
        match mark {
            MinimapMark::Track => {
                cell.set_char('│');
                cell.set_style(theme::minimap_track_style());
            }
            MinimapMark::Viewport => {
                cell.set_char('│');
                cell.set_style(theme::minimap_viewport_style());
            }
            MinimapMark::Highlight => {
                cell.set_char('•');
                cell.set_style(theme::minimap_highlight_style());
            }
            MinimapMark::Bookmark => {
                cell.set_char('•');
                cell.set_style(Style::default().fg(theme::bookmark_minimap_color()));
            }
            MinimapMark::Severe => {
                cell.set_char('•');
                cell.set_style(theme::minimap_severe_style());
            }
        }
    }
}

/// Takes `&mut App` (unlike sibling `render_*` functions) so ratatui's
/// scroll offset can be persisted across frames via `App.list_offset` —
/// do not revert this to `&App`, that's exactly what caused the old
/// viewport-snap bug.
pub fn render_log_list(app: &mut App, frame: &mut Frame, area: Rect) {
    let active = app.focus == Focus::LogList;
    let loading = app.log_loading_label();
    let title = theme::numbered_title_with_loading(4, "Log", active, loading.as_deref());
    let block = rounded_block(title, active);
    let inner = block.inner(area);
    // H3: reserve 1 inner column for the minimap when there is content.
    let rail_w = if !app.index.visible.is_empty() && inner.width > 1 {
        1u16
    } else {
        0
    };
    let content_w = inner.width.saturating_sub(rail_w).max(1);
    let inner_width = content_w as usize;
    let selection = app.selection_range();
    let patterns = app.highlight_groups.paint_patterns(app.active_highlight);

    let lineno_width = app.index.visible.len().max(1).to_string().len();

    // Compute list_area before building items for the virtual-scroll window size.
    // block.inner() is a pure rect computation — it does not render anything.
    let content_area = Rect {
        x: inner.x,
        y: inner.y,
        width: content_w,
        height: inner.height,
    };
    // M2: one-line compare-tray summary at top of Log (folded when empty).
    let bm_lines = if app.bookmarks.is_empty() || content_area.height <= 1 {
        Vec::new()
    } else {
        vec![bookmark_summary_line(&app.bookmarks.summary_line())]
    };
    let bm_h = bm_lines.len() as u16;
    let (bm_area_opt, list_area) = if bm_h > 0 && content_area.height > bm_h {
        let [top, rest] =
            Layout::vertical([Constraint::Length(bm_h), Constraint::Fill(1)]).areas(content_area);
        (Some((top, bm_lines)), rest)
    } else {
        (None, content_area)
    };

    // ── Virtual scroll ─────────────────────────────────────────────────────────
    // Build ListItems only for a window around the current scroll position.
    // 3× the viewport height provides a safe margin for multi-line entries.
    let n = app.index.visible.len();
    let viewport_h = (list_area.height as usize).max(1);
    let window_size = (viewport_h * 3).max(20);

    // Align window so the cursor is always inside it:
    //  • cursor above list_offset  → slide window up to cursor (smooth k scrolling)
    //  • cursor past window bottom → anchor at cursor − viewport_h (G / follow append)
    //  • otherwise                 → keep window at list_offset
    let window_start = if n == 0 {
        0
    } else if app.cursor < app.list_offset {
        app.cursor
    } else if app.cursor >= app.list_offset.saturating_add(window_size) {
        app.cursor.saturating_sub(viewport_h)
    } else {
        app.list_offset
    };
    let window_start = window_start.min(n.saturating_sub(1));
    let window_end = (window_start + window_size).min(n);

    // cursor position relative to the window (always in-bounds after alignment).
    let rel_cursor = if n > 0 {
        app.cursor.saturating_sub(window_start)
    } else {
        0
    };

    let items: Vec<ListItem> = if n == 0 {
        Vec::new()
    } else {
        (window_start..window_end)
            .filter_map(|abs_i| {
                let row = app.row_at(abs_i)?;
                let pinned = app.is_bookmark_row(row.row_id);
                let lines = if app.collapsed_view {
                    vec![render_entry_line_collapsed(
                        &row,
                        &patterns,
                        inner_width,
                        abs_i + 1,
                        lineno_width,
                        pinned,
                    )]
                } else {
                    render_entry_lines_ex(
                        &row,
                        &patterns,
                        inner_width,
                        Some((abs_i + 1, lineno_width)),
                        false,
                        pinned,
                    )
                };
                let mut item = ListItem::new(lines);
                if let Some((lo, hi)) = selection {
                    if abs_i >= lo && abs_i <= hi {
                        item = item.style(theme::log_visual_style());
                    } else if pinned {
                        item = item.style(theme::bookmark_row_style());
                    }
                } else if active && abs_i == app.cursor {
                    item = item.style(theme::log_selection_style());
                } else if pinned {
                    item = item.style(theme::bookmark_row_style());
                }
                Some(item)
            })
            .collect()
    };

    // Paint border first; list fills the content columns only (no block).
    frame.render_widget(block, area);
    if let Some((area, lines)) = bm_area_opt {
        render_bookmark_strip(frame, area, lines);
    }

    // rel_offset is always 0: window_start == list_offset in the stable case,
    // so the relative offset within the window is 0. ratatui computes the final
    // scroll position (state.offset()) to keep rel_cursor visible, and we store
    // the absolute result back into app.list_offset below.
    let list = List::new(items);
    let mut state = ListState::default().with_offset(0);
    if n > 0 {
        state.select(Some(rel_cursor));
    }
    frame.render_stateful_widget(list, list_area, &mut state);
    // Restore absolute offset: window_start + what ratatui settled on.
    app.list_offset = window_start + state.offset();

    if rail_w > 0 && inner.height > 0 {
        let rail = Rect {
            x: inner.x.saturating_add(content_w),
            y: inner.y,
            width: 1,
            height: inner.height,
        };
        render_minimap(app, frame, rail);
    }
}

/// One-line Log-top compare-tray summary.
pub(super) fn bookmark_summary_line(text: &str) -> Line<'static> {
    let painted = text.replacen('★', theme::GLYPH_BOOKMARK_PIN, 1);
    Line::from(Span::styled(
        format!(" {painted}"),
        theme::bookmark_label_style(),
    ))
}

/// M2: one summary line inside the Log region.
pub fn render_bookmark_strip(frame: &mut Frame, area: Rect, lines: Vec<Line<'static>>) {
    if area.height == 0 {
        return;
    }
    frame.render_widget(Block::default().style(theme::bookmark_strip_style()), area);
    let shown: Vec<Line<'static>> = lines.into_iter().take(area.height as usize).collect();
    frame.render_widget(Paragraph::new(shown), area);
}
