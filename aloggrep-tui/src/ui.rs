use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph};
use ratatui::Frame;
use regex::Regex;

use crate::app::{App, Focus, Mode};
use crate::filter_model::Group;
use crate::input::InputBox;
use crate::model::EntryRow;
use crate::search_model::{SearchBox, SearchGroup};
use crate::theme;

/// Horizontal gap (columns) between chip groups on the same wrap row.
const CHIP_GROUP_GAP: u16 = 1;
/// Gap between the selection marker and the group's pills.
const DOT_PILL_GAP: u16 = 1;
/// Gap between adjacent pills inside a group.
const PILL_GAP: u16 = 1;
/// Shared centered-modal width: leave 2 cols margin each side, clamp to a
/// readable band so Input and Search share one visual scale.
pub const MODAL_WIDTH_MIN: u16 = 24;
pub const MODAL_WIDTH_MAX: u16 = 56;

fn rounded_block(title: Line<'static>, active: bool) -> Block<'static> {
    Block::new()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(theme::border_style(active))
        .title(title)
}

/// Unified width for centered Input / Search modals.
pub fn modal_width(frame_width: u16) -> u16 {
    frame_width.saturating_sub(4).clamp(MODAL_WIDTH_MIN, MODAL_WIDTH_MAX)
}

/// Horizontally and vertically center a `width`×`height` rect inside `frame`.
pub fn centered_modal_rect(frame: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(frame.width).max(1);
    let height = height.min(frame.height).max(1);
    let x = frame.x + (frame.width.saturating_sub(width)) / 2;
    let y = frame.y + (frame.height.saturating_sub(height)) / 2;
    Rect {
        x,
        y,
        width,
        height,
    }
}

/// Horizontally centered, vertically near the top (H1 Input/Search stack).
pub fn top_modal_rect(frame: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(frame.width).max(1);
    let height = height.min(frame.height).max(1);
    let x = frame.x + (frame.width.saturating_sub(width)) / 2;
    let y = frame.y.saturating_add(1).min(
        frame
            .y
            .saturating_add(frame.height.saturating_sub(height)),
    );
    Rect {
        x,
        y,
        width,
        height,
    }
}

/// Place a rect of `height` directly below `anchor`, clamped to `frame`.
pub fn stack_below_rect(anchor: Rect, frame: Rect, height: u16) -> Rect {
    let y = anchor.y.saturating_add(anchor.height);
    let frame_bottom = frame.y.saturating_add(frame.height);
    let space = frame_bottom.saturating_sub(y);
    let height = height.min(space).max(if space > 0 { 1 } else { 0 });
    Rect {
        x: anchor.x,
        y,
        width: anchor.width,
        height,
    }
}

/// Clear + rounded active shell with a plain title. Returns the inner content rect.
pub fn render_modal_shell(title: &str, frame: &mut Frame, area: Rect) -> Rect {
    frame.render_widget(Clear, area);
    let block = rounded_block(theme::plain_title(title, true), true);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(Clear, inner);
    inner
}

/// Candidate list skin shared by field popup and Search history completion.
pub fn render_candidate_list(
    title: &str,
    labels: &[String],
    styles: &[Style],
    selected: usize,
    empty_msg: &str,
    frame: &mut Frame,
    area: Rect,
) {
    frame.render_widget(Clear, area);
    let block = rounded_block(theme::plain_title(title, true), true);
    if labels.is_empty() {
        let inner = block.inner(area);
        frame.render_widget(block, area);
        frame.render_widget(
            Paragraph::new(Span::styled(empty_msg, Style::default().add_modifier(Modifier::DIM))),
            inner,
        );
        return;
    }
    let items: Vec<ListItem> = labels
        .iter()
        .zip(styles.iter())
        .map(|(label, style)| ListItem::new(Span::styled(format!(" {label} "), *style)))
        .collect();
    let list = List::new(items)
        .block(block)
        .highlight_style(theme::candidate_selection_style())
        .highlight_symbol("\u{203a} ");
    let mut state = ListState::default();
    state.select(Some(selected.min(labels.len() - 1)));
    frame.render_stateful_widget(list, area, &mut state);
}

/// Candidate popup height: `clamp(count,1,8)+2` for border, clamped to
/// space below the modal anchor (Input / Search / H7 msg share this).
pub fn candidate_popup_rect(anchor: Rect, frame: Rect, match_count: usize) -> Rect {
    let desired = match_count.clamp(1, 8) as u16 + 2;
    stack_below_rect(anchor, frame, desired)
}

/// H1 Preview window height: content rows + border, clamped to space below
/// the previous stack item (candidates or modal).
pub fn preview_popup_rect(anchor: Rect, frame: Rect, content_rows: usize) -> Rect {
    let desired = (content_rows.clamp(1, 12) as u16).saturating_add(2);
    stack_below_rect(anchor, frame, desired)
}

/// Search modal outer height: draft row + borders (candidates float below).
pub fn search_modal_height() -> u16 {
    3
}

/// Greedy word-wrap: returns byte ranges into `text`, one per physical
/// line, breaking on whitespace where possible. A single word longer than
/// `width` is hard-cut into `width`-sized pieces (never infinite-loops).
/// Leading/trailing whitespace around words is dropped; interior spacing
/// between words on the same line is preserved verbatim.
fn wrap_ranges(text: &str, width: usize) -> Vec<(usize, usize)> {
    let width = width.max(1);
    if text.is_empty() {
        return vec![(0, 0)];
    }

    let mut words: Vec<(usize, usize)> = Vec::new();
    let mut word_start: Option<usize> = None;
    for (i, ch) in text.char_indices() {
        if ch.is_whitespace() {
            if let Some(s) = word_start.take() {
                words.push((s, i));
            }
        } else if word_start.is_none() {
            word_start = Some(i);
        }
    }
    if let Some(s) = word_start {
        words.push((s, text.len()));
    }
    if words.is_empty() {
        return vec![(0, text.len())];
    }

    let mut ranges = Vec::new();
    let mut line_start = words[0].0;
    let mut line_end = words[0].0;

    for &(ws, we) in &words {
        let word_chars = text[ws..we].chars().count();
        if word_chars > width {
            if line_end > line_start {
                ranges.push((line_start, line_end));
            }
            let char_offsets: Vec<usize> =
                text[ws..we].char_indices().map(|(i, _)| ws + i).chain(std::iter::once(we)).collect();
            let mut c = 0usize;
            while c < char_offsets.len() - 1 {
                let take = width.min(char_offsets.len() - 1 - c);
                ranges.push((char_offsets[c], char_offsets[c + take]));
                c += take;
            }
            line_start = we;
            line_end = we;
            continue;
        }

        if line_end == line_start {
            line_end = we; // first word on an empty line always fits (checked above)
            continue;
        }
        let candidate_chars = text[line_start..we].chars().count();
        if candidate_chars > width {
            ranges.push((line_start, line_end));
            line_start = ws;
            line_end = we;
        } else {
            line_end = we;
        }
    }
    if line_end > line_start || ranges.is_empty() {
        ranges.push((line_start, line_end.max(line_start)));
    }
    ranges
}

/// Match segment: start, end, progressive color index, globally-active underline.
type ColoredMatch = (usize, usize, usize, bool);

/// Paint pattern: regex, color index, whether this is the globally active search.
type PaintPattern<'a> = (&'a Regex, usize, bool);

/// Collect all pattern matches; later patterns overwrite overlapping ranges
/// (same order as `paint_patterns`).
fn collect_matches(msg: &str, patterns: &[PaintPattern<'_>]) -> Vec<ColoredMatch> {
    // Per-byte: (color_idx, is_active)
    let mut marked: Vec<Option<(usize, bool)>> = vec![None; msg.len()];
    for &(re, color_idx, is_active) in patterns {
        for m in re.find_iter(msg) {
            for i in m.start()..m.end() {
                marked[i] = Some((color_idx, is_active));
            }
        }
    }
    let mut out = Vec::new();
    let mut i = 0;
    while i < marked.len() {
        if let Some((color, active)) = marked[i] {
            let start = i;
            i += 1;
            while i < marked.len() && marked[i] == Some((color, active)) {
                i += 1;
            }
            // Only emit on char boundaries — marked is per-byte; regex matches
            // are already on char boundaries for UTF-8.
            out.push((start, i, color, active));
        } else {
            i += 1;
        }
    }
    out
}

/// Splits `text[range.0..range.1]` into plain/highlighted spans.
/// Non-matched segments use `base`.
fn spans_for_range(
    text: &str,
    range: (usize, usize),
    matches: &[ColoredMatch],
    base: Style,
) -> Vec<Span<'static>> {
    let (start, end) = range;
    let mut spans = Vec::new();
    let mut cursor = start;
    for &(m_start, m_end, color_idx, is_active) in matches {
        if m_end <= start || m_start >= end {
            continue;
        }
        let seg_start = m_start.max(start);
        let seg_end = m_end.min(end);
        if seg_start > cursor {
            spans.push(Span::styled(text[cursor..seg_start].to_string(), base));
        }
        let style = if is_active {
            theme::highlight_style_active(color_idx)
        } else {
            theme::highlight_style(color_idx)
        };
        spans.push(Span::styled(text[seg_start..seg_end].to_string(), style));
        cursor = seg_end;
    }
    if cursor < end {
        spans.push(Span::styled(text[cursor..end].to_string(), base));
    }
    spans
}

/// Renders one log entry as one or more physical `Line`s: a header
/// (visible lineno/timestamp/level/tag) followed by the message, word-wrapped
/// to `area_width` instead of being truncated. Fields use natural character
/// widths (no fixed column padding); continuation lines indent with spaces
/// matching the header width so the message column stays aligned.
/// `lineno` is 1-based within the visible set; `lineno_width` is the digit
/// width used for right-aligned padding.
fn render_entry_lines(
    row: &EntryRow,
    patterns: &[PaintPattern<'_>],
    area_width: usize,
    lineno: usize,
    lineno_width: usize,
) -> Vec<Line<'static>> {
    let lineno_s = format!("{lineno:>lineno_width$} ");
    let ts = format!("{} ", row.timestamp);
    let level_badge = format!(" {} ", row.level.as_char());
    let tag_display = format!("{} ", row.tag);
    let header_width = lineno_s.chars().count()
        + ts.chars().count()
        + level_badge.chars().count()
        + tag_display.chars().count();
    let cont_prefix: String = " ".repeat(header_width);

    let first_width = area_width.saturating_sub(header_width).max(8);
    let cont_width = area_width.saturating_sub(header_width).max(8);

    let tag_style = Style::default().fg(theme::accent()).add_modifier(Modifier::BOLD);
    let tag_matches = collect_matches(&row.tag, patterns);
    let msg_matches = collect_matches(&row.msg, patterns);

    let first_pass = wrap_ranges(&row.msg, first_width);
    let mut line_ranges: Vec<(usize, usize)> = vec![first_pass[0]];
    let first_end = first_pass[0].1;
    if first_end < row.msg.len() {
        for (s, e) in wrap_ranges(&row.msg[first_end..], cont_width) {
            line_ranges.push((first_end + s, first_end + e));
        }
    }

    line_ranges
        .into_iter()
        .enumerate()
        .map(|(i, range)| {
            let mut spans = Vec::new();
            if i == 0 {
                spans.push(Span::styled(
                    lineno_s.clone(),
                    theme::muted().add_modifier(Modifier::DIM),
                ));
                spans.push(Span::styled(ts.clone(), theme::muted()));
                spans.push(Span::styled(level_badge.clone(), theme::level_badge_style(row.level)));
                if tag_matches.is_empty() {
                    spans.push(Span::styled(tag_display.clone(), tag_style));
                } else {
                    spans.extend(spans_for_range(
                        &row.tag,
                        (0, row.tag.len()),
                        &tag_matches,
                        tag_style,
                    ));
                    spans.push(Span::styled(" ", tag_style));
                }
            } else {
                spans.push(Span::styled(cont_prefix.clone(), Style::default().add_modifier(Modifier::DIM)));
            }
            spans.extend(spans_for_range(&row.msg, range, &msg_matches, Style::default()));
            Line::from(spans)
        })
        .collect()
}

/// H3 minimap cell priority (higher wins on overlap).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum MinimapMark {
    Track = 0,
    Viewport = 1,
    Search = 2,
    Severe = 3,
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
    if h == 0 || app.visible.is_empty() {
        return Vec::new();
    }
    let n = app.visible.len();
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

    let samples = n.min(MINIMAP_MARK_BUDGET);
    for s in 0..samples {
        let i = if samples <= 1 {
            0
        } else {
            s * (n - 1) / (samples - 1)
        };
        let row = &app.rows[app.visible[i]];
        let r = minimap_row_for_index(i, n, h);
        if app.search_groups.any_match(&row.tag, &row.msg) && cells[r] < MinimapMark::Search {
            cells[r] = MinimapMark::Search;
        }
        if crate::app::is_severe_row(row) {
            cells[r] = MinimapMark::Severe;
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
            MinimapMark::Search => {
                cell.set_char('•');
                cell.set_style(theme::minimap_search_style());
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
    let block = rounded_block(theme::numbered_title(4, "Log", active), active);
    let inner = block.inner(area);
    // H3: reserve 1 inner column for the minimap when there is content.
    let rail_w = if !app.visible.is_empty() && inner.width > 1 {
        1u16
    } else {
        0
    };
    let content_w = inner.width.saturating_sub(rail_w).max(1);
    let inner_width = content_w as usize;
    let selection = app.selection_range();
    let patterns = app.search_groups.paint_patterns(app.active_search);

    let lineno_width = app.visible.len().max(1).to_string().len();
    let items: Vec<ListItem> = app
        .visible_rows()
        .enumerate()
        .map(|(i, row)| {
            let mut item = ListItem::new(render_entry_lines(row, &patterns, inner_width, i + 1, lineno_width));
            if let Some((lo, hi)) = selection {
                if i >= lo && i <= hi {
                    item = item.style(theme::log_visual_style());
                }
            } else if active && i == app.cursor {
                // Apply selection via ListItem so Span highlight bg is not
                // overwritten by List::highlight_style's Style::patch.
                item = item.style(theme::log_selection_style());
            }
            item
        })
        .collect();
    // Paint border first; list fills the content columns only (no block).
    frame.render_widget(block, area);
    let content_area = Rect {
        x: inner.x,
        y: inner.y,
        width: content_w,
        height: inner.height,
    };
    // M2: bookmark strip embedded at top of Log (collapsed when empty).
    let bm_n = app.bookmarks.display_recent().len() as u16;
    let (bm_area, list_area) = if bm_n > 0 && content_area.height > bm_n {
        let [top, rest] = Layout::vertical([
            Constraint::Length(bm_n),
            Constraint::Fill(1),
        ])
        .areas(content_area);
        (Some(top), rest)
    } else {
        (None, content_area)
    };
    if let Some(area) = bm_area {
        render_bookmark_strip(app, frame, area);
    }
    // No List::highlight_style — selection is painted on the item above.
    let list = List::new(items);
    let mut state = ListState::default().with_offset(app.list_offset);
    if !app.visible.is_empty() {
        state.select(Some(app.cursor));
    }
    frame.render_stateful_widget(list, list_area, &mut state);
    app.list_offset = state.offset();

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

/// M2: up to 3 newest bookmarks inside the Log region.
pub fn render_bookmark_strip(app: &App, frame: &mut Frame, area: Rect) {
    if area.height == 0 {
        return;
    }
    frame.render_widget(
        Block::default().style(theme::bookmark_strip_style()),
        area,
    );
    let recent = app.bookmarks.display_recent();
    let lines: Vec<Line> = recent
        .iter()
        .take(area.height as usize)
        .map(|bm| {
            let alive = app.bookmark_alive(bm.row_id);
            let style = if alive {
                theme::bookmark_label_style()
            } else {
                theme::bookmark_stale_style()
            };
            let mark = if alive { "★" } else { "☆" };
            Line::from(Span::styled(format!(" {mark} {}", bm.label), style))
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), area);
}

/// M2 `mm` picker: draft + filtered bookmark list (newest first).
pub fn render_bookmark_picker(app: &App, frame: &mut Frame, area: Rect) {
    let Some(picker) = &app.bookmark_picker else {
        return;
    };
    let inner = render_modal_shell("Bookmarks", frame, area);
    if inner.height == 0 {
        return;
    }
    let [draft_area, list_area] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Fill(1),
    ])
    .areas(inner);
    let draft_spans = vec![
        Span::styled("filter ", theme::muted()),
        Span::styled(picker.draft.clone(), Style::reset()),
        theme::caret_bar(),
    ];
    frame.render_widget(Paragraph::new(Line::from(draft_spans)), draft_area);

    let filtered = picker.filtered_indices(&app.bookmarks);
    if filtered.is_empty() {
        frame.render_widget(
            Paragraph::new(Span::styled(
                "无书签".to_string(),
                theme::preview_placeholder_style(),
            )),
            list_area,
        );
        return;
    }
    let labels: Vec<String> = filtered
        .iter()
        .map(|&i| {
            let bm = &app.bookmarks.items[i];
            if app.bookmark_alive(bm.row_id) {
                format!("★ {}", bm.label)
            } else {
                format!("☆ {} (失效)", bm.label)
            }
        })
        .collect();
    let styles: Vec<Style> = filtered
        .iter()
        .map(|&i| {
            let bm = &app.bookmarks.items[i];
            if app.bookmark_alive(bm.row_id) {
                theme::bookmark_label_style()
            } else {
                theme::bookmark_stale_style()
            }
        })
        .collect();
    let selected = picker.selected.min(labels.len() - 1);
    render_candidate_list("jump", &labels, &styles, selected, "无匹配", frame, list_area);
}

/// Height for the `mm` modal (draft + up to 8 rows + border).
pub fn bookmark_picker_height(match_count: usize) -> u16 {
    let rows = match_count.clamp(1, 8) as u16;
    rows + 1 + 2 // draft + border
}

fn group_dot_span(enabled: bool, selected: bool) -> Span<'static> {
    let dot = if enabled { '●' } else { '○' };
    // Selection uses the same Magenta accent as region selection frames;
    // kept to one cell so the strip can stay a single content row tall.
    let style = if selected {
        theme::chip_group_border_style(true).add_modifier(Modifier::BOLD)
    } else if !enabled {
        theme::disabled_chip_style()
    } else {
        Style::default().add_modifier(Modifier::DIM)
    };
    Span::styled(dot.to_string(), style)
}

fn filter_group_spans(g: &Group, selected: bool) -> Vec<Span<'static>> {
    let mut spans = vec![group_dot_span(g.enabled, selected)];
    spans.push(Span::raw(" ".repeat(DOT_PILL_GAP as usize)));
    if g.chips.is_empty() {
        let style = if !g.enabled {
            theme::disabled_chip_style()
        } else {
            Style::default()
        };
        spans.push(Span::styled(format!(" {} ", g.label), style));
    } else {
        for (i, chip) in g.chips.iter().enumerate() {
            if i > 0 {
                spans.push(Span::raw(" ".repeat(PILL_GAP as usize)));
            }
            let (text, body) = theme::chip_pill_style(chip.field, &chip.value, !g.enabled);
            spans.push(Span::styled(text, body));
        }
    }
    spans
}

fn search_group_spans(
    g: &SearchGroup,
    color_idx: usize,
    selected: bool,
    active_global: bool,
) -> Vec<Span<'static>> {
    let mut spans = vec![group_dot_span(g.enabled, selected)];
    spans.push(Span::raw(" ".repeat(DOT_PILL_GAP as usize)));
    let (text, body) = theme::search_pill_style(&g.pattern, color_idx, !g.enabled, active_global);
    spans.push(Span::styled(text, body));
    spans
}

fn exclude_entry_spans(e: &crate::filter_model::ExcludeEntry, selected: bool) -> Vec<Span<'static>> {
    let mut spans = vec![group_dot_span(e.enabled, selected)];
    spans.push(Span::raw(" ".repeat(DOT_PILL_GAP as usize)));
    let (text, body) = theme::exclude_pill_style(e.chip.field, &e.chip.value, !e.enabled);
    spans.push(Span::styled(text, body));
    spans
}

fn span_width(span: &Span<'_>) -> usize {
    span.content.chars().count()
}

fn wrap_spans(spans: Vec<Span<'static>>, width: usize) -> Vec<Line<'static>> {
    let width = width.max(1);
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut current: Vec<Span<'static>> = Vec::new();
    let mut used = 0usize;

    for span in spans {
        let w = span_width(&span);
        if w > width {
            if !current.is_empty() {
                lines.push(Line::from(std::mem::take(&mut current)));
                used = 0;
            }
            let text = span.content.as_ref().to_string();
            let style = span.style;
            let chars: Vec<char> = text.chars().collect();
            let mut i = 0;
            while i < chars.len() {
                let end = (i + width).min(chars.len());
                let chunk: String = chars[i..end].iter().collect();
                lines.push(Line::from(Span::styled(chunk, style)));
                i = end;
            }
            continue;
        }
        if !current.is_empty() && used + w > width {
            lines.push(Line::from(std::mem::take(&mut current)));
            used = 0;
        }
        used += w;
        current.push(span);
    }
    if !current.is_empty() || lines.is_empty() {
        lines.push(Line::from(current));
    }
    lines
}

fn flow_wrap_groups(groups: Vec<Vec<Span<'static>>>, width: u16) -> Vec<Line<'static>> {
    let width = width.max(1) as usize;
    let mut out: Vec<Line<'static>> = Vec::new();
    let mut row_spans: Vec<Span<'static>> = Vec::new();
    let mut used = 0usize;

    for group in groups {
        let group_w: usize = group.iter().map(span_width).sum();
        if group_w > width {
            if !row_spans.is_empty() {
                out.push(Line::from(std::mem::take(&mut row_spans)));
                used = 0;
            }
            out.extend(wrap_spans(group, width));
            continue;
        }
        let need = if row_spans.is_empty() {
            group_w
        } else {
            CHIP_GROUP_GAP as usize + group_w
        };
        if !row_spans.is_empty() && used + need > width {
            out.push(Line::from(std::mem::take(&mut row_spans)));
            used = 0;
        }
        if !row_spans.is_empty() {
            row_spans.push(Span::raw(" ".repeat(CHIP_GROUP_GAP as usize)));
            used += CHIP_GROUP_GAP as usize;
        }
        used += group_w;
        row_spans.extend(group);
    }
    if !row_spans.is_empty() {
        out.push(Line::from(row_spans));
    }
    out
}

fn filter_strip_lines(app: &App, inner_width: u16) -> Vec<Line<'static>> {
    let active = app.focus == Focus::ChipStrip;
    let groups: Vec<Vec<Span<'static>>> = app
        .groups
        .groups
        .iter()
        .enumerate()
        .map(|(i, g)| filter_group_spans(g, i == app.group_cursor && active))
        .collect();
    flow_wrap_groups(groups, inner_width)
}

fn search_strip_lines(app: &App, inner_width: u16) -> Vec<Line<'static>> {
    let active = app.focus == Focus::SearchStrip;
    let mut color_idx = 0usize;
    let groups: Vec<Vec<Span<'static>>> = app
        .search_groups
        .groups
        .iter()
        .enumerate()
        .map(|(i, g)| {
            let idx = if g.enabled {
                let c = color_idx;
                color_idx += 1;
                c
            } else {
                0
            };
            search_group_spans(
                g,
                idx,
                i == app.search_cursor && active,
                Some(i) == app.active_search,
            )
        })
        .collect();
    flow_wrap_groups(groups, inner_width)
}

fn exclude_strip_lines(app: &App, inner_width: u16) -> Vec<Line<'static>> {
    let active = app.focus == Focus::ExcludeStrip;
    let groups: Vec<Vec<Span<'static>>> = app
        .groups
        .excludes
        .iter()
        .enumerate()
        .map(|(i, e)| exclude_entry_spans(e, i == app.exclude_cursor && active))
        .collect();
    flow_wrap_groups(groups, inner_width)
}

/// Strip height: `0` when empty, else `2` (rounded region chrome) + content
/// rows. Content is a single terminal row per wrap line — nested per-chip
/// `Block`s need 3 rows each and made the strip ~2× taller than a cell's
/// visual proportions allow.
pub fn filter_strip_height(app: &App, outer_width: u16) -> u16 {
    if app.groups.groups.is_empty() {
        return 0;
    }
    let inner = outer_width.saturating_sub(2);
    let rows = filter_strip_lines(app, inner).len().max(1) as u16;
    rows.saturating_add(2)
}

/// Same rules as [`filter_strip_height`] for the Exclude strip (H9).
pub fn exclude_strip_height(app: &App, outer_width: u16) -> u16 {
    if app.groups.excludes.is_empty() {
        return 0;
    }
    let inner = outer_width.saturating_sub(2);
    let rows = exclude_strip_lines(app, inner).len().max(1) as u16;
    rows.saturating_add(2)
}

/// Same rules as [`filter_strip_height`] for the Search strip.
pub fn search_strip_height(app: &App, outer_width: u16) -> u16 {
    if app.search_groups.groups.is_empty() {
        return 0;
    }
    let inner = outer_width.saturating_sub(2);
    let rows = search_strip_lines(app, inner).len().max(1) as u16;
    rows.saturating_add(2)
}

pub fn render_chip_strip(app: &App, frame: &mut Frame, area: Rect) {
    if area.height == 0 {
        return;
    }
    let active = app.focus == Focus::ChipStrip;
    let block = rounded_block(theme::numbered_title(1, "Filter", active), active);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(Paragraph::new(filter_strip_lines(app, inner.width)), inner);
}

pub fn render_exclude_chip_strip(app: &App, frame: &mut Frame, area: Rect) {
    if area.height == 0 {
        return;
    }
    let active = app.focus == Focus::ExcludeStrip;
    let block = rounded_block(theme::numbered_title(2, "Exclude", active), active);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(Paragraph::new(exclude_strip_lines(app, inner.width)), inner);
}

pub fn render_search_chip_strip(app: &App, frame: &mut Frame, area: Rect) {
    if area.height == 0 {
        return;
    }
    let active = app.focus == Focus::SearchStrip;
    let block = rounded_block(theme::numbered_title(3, "Search", active), active);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(Paragraph::new(search_strip_lines(app, inner.width)), inner);
}

fn input_content_spans(input: &InputBox, show_caret: bool) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    for (i, chip) in input.chips.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw(" ".repeat(PILL_GAP as usize)));
        }
        let (text, body) = if input.exclude_mode {
            theme::exclude_pill_style(chip.field, &chip.value, false)
        } else {
            theme::chip_pill_style(chip.field, &chip.value, false)
        };
        spans.push(Span::styled(text, body));
    }
    // Gap + reset after pills so draft/caret never sit inside the pill fill.
    if !input.chips.is_empty() {
        spans.push(Span::styled(" ".repeat(PILL_GAP as usize), Style::reset()));
    }
    if let Some(field) = input.draft_field {
        spans.push(Span::styled(
            format!("{}:", field.keyword()),
            Style::reset().fg(theme::field_color(field)),
        ));
    }
    spans.push(Span::styled(input.draft.clone(), Style::reset()));
    if show_caret {
        spans.push(theme::caret_bar());
    }
    spans
}

/// Centered Input modal (visible while `Focus::Input`).
pub fn render_input_modal(input: &InputBox, mode: Mode, frame: &mut Frame, area: Rect) {
    let title = if input.exclude_mode {
        "Input ! (排除)"
    } else {
        "Input"
    };
    let inner = render_modal_shell(title, frame, area);
    frame.render_widget(
        Paragraph::new(Line::from(input_content_spans(input, mode == Mode::Insert))),
        inner,
    );
}

/// Legacy single-row Input render kept for unit tests that draw into a fixed area.
pub fn render_input_box(input: &InputBox, mode: Mode, focused: bool, frame: &mut Frame, area: Rect) {
    let block = rounded_block(theme::numbered_title(5, "Input", focused), focused);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(Clear, inner);
    frame.render_widget(
        Paragraph::new(Line::from(input_content_spans(input, mode == Mode::Insert))),
        inner,
    );
}

/// Centered Search modal: draft row only (history candidates float below).
pub fn render_search_modal(search: &SearchBox, frame: &mut Frame, area: Rect) {
    let inner = render_modal_shell("Search", frame, area);
    let spans = vec![
        Span::styled(
            "/",
            Style::reset().fg(theme::accent()).add_modifier(Modifier::BOLD),
        ),
        Span::styled(search.draft.clone(), Style::reset()),
        theme::caret_bar(),
    ];
    frame.render_widget(Paragraph::new(Line::from(spans)), inner);
}

/// Outer height for H4 Detail modal (border + content), clamped to `frame`.
pub fn detail_modal_height(frame: Rect, content_rows: usize) -> u16 {
    let desired = (content_rows as u16).saturating_add(2).max(3);
    let max = frame.height.saturating_mul(3) / 5;
    let max = max.max(5).min(frame.height.saturating_sub(1));
    desired.min(max).max(3)
}

/// Build H4 Fields-mode lines for the current row (used by render + height).
pub fn detail_field_lines(row: Option<&crate::model::EntryRow>, inner_width: u16) -> Vec<Line<'static>> {
    use crate::input::ChipField;

    let label_w = 5usize;
    let value_w = inner_width.saturating_sub(label_w as u16 + 1).max(1) as usize;

    let mut lines: Vec<Line<'static>> = Vec::new();
    let Some(row) = row else {
        lines.push(Line::from(Span::styled(
            "无选中行".to_string(),
            theme::preview_placeholder_style(),
        )));
        return lines;
    };

    let push_kv = |lines: &mut Vec<Line<'static>>, label: &str, label_style: Style, value: String, value_style: Style| {
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
pub fn detail_pretty_lines(row: Option<&crate::model::EntryRow>, inner_width: u16) -> Vec<Line<'static>> {
    let width = inner_width.max(1) as usize;
    let mut lines: Vec<Line<'static>> = Vec::new();
    let Some(row) = row else {
        lines.push(Line::from(Span::styled(
            "无选中行".to_string(),
            theme::preview_placeholder_style(),
        )));
        return lines;
    };
    let (text, is_json) = pretty_json_for_row(row);
    if !is_json {
        lines.push(Line::from(Span::styled(
            "非 JSON".to_string(),
            theme::preview_placeholder_style(),
        )));
    }
    for (s, e) in wrap_ranges(&text, width) {
        lines.push(Line::from(Span::raw(text[s..e].to_string())));
    }
    lines
}

/// Content lines for the current detail mode (height estimation + render).
pub fn detail_content_lines(app: &App, inner_width: u16) -> Vec<Line<'static>> {
    use crate::app::DetailView;
    match app.detail {
        DetailView::Fields => detail_field_lines(app.current_row(), inner_width),
        DetailView::Pretty => detail_pretty_lines(app.current_row(), inner_width),
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

/// H1 Preview window: sampled result lines (Filter) or faint-highlighted hits (Search).
pub fn render_preview(
    title: &str,
    lines: &[crate::preview::PreviewLine],
    placeholder: &str,
    frame: &mut Frame,
    area: Rect,
) {
    if area.height == 0 {
        return;
    }
    let inner = render_modal_shell(title, frame, area);
    if lines.is_empty() {
        frame.render_widget(
            Paragraph::new(Span::styled(
                placeholder.to_string(),
                theme::preview_placeholder_style(),
            )),
            inner,
        );
        return;
    }
    let items: Vec<ListItem> = lines
        .iter()
        .map(|line| {
            if let Some((s, e)) = line.highlight {
                let s = s.min(line.text.len());
                let e = e.min(line.text.len()).max(s);
                ListItem::new(Line::from(vec![
                    Span::raw(line.text[..s].to_string()),
                    Span::styled(line.text[s..e].to_string(), theme::preview_highlight_style()),
                    Span::raw(line.text[e..].to_string()),
                ]))
            } else {
                ListItem::new(Span::raw(line.text.clone()))
            }
        })
        .collect();
    frame.render_widget(List::new(items), inner);
}

/// H7 `c`+`m` draft modal (candidates float below via [`render_msg_chip_popup`]).
pub fn render_msg_chip_modal(picker: &crate::input::MsgChipPicker, frame: &mut Frame, area: Rect) {
    let inner = render_modal_shell("msg chip", frame, area);
    let spans = vec![
        Span::styled(
            "msg~",
            Style::reset()
                .fg(theme::field_color(crate::input::ChipField::Msg))
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(picker.draft.clone(), Style::reset()),
        theme::caret_bar(),
    ];
    frame.render_widget(Paragraph::new(Line::from(spans)), inner);
}

/// H7 msg-token candidates — same shell as Input field / Search history popup.
pub fn render_msg_chip_popup(
    picker: &crate::input::MsgChipPicker,
    frame: &mut Frame,
    area: Rect,
) {
    let candidates = picker.candidates();
    let n = candidates.len().min(8);
    let labels: Vec<String> = candidates.iter().take(n).map(|s| (*s).to_string()).collect();
    let styles: Vec<Style> = (0..n)
        .map(|_| Style::default().fg(theme::field_color(crate::input::ChipField::Msg)))
        .collect();
    let selected = if n == 0 {
        0
    } else {
        picker.selected.min(n - 1)
    };
    render_candidate_list("片段", &labels, &styles, selected, "无匹配片段", frame, area);
}

/// Search history-chip candidates — same shell as Input field popup.
pub fn render_search_popup(
    search: &SearchBox,
    groups: &[SearchGroup],
    frame: &mut Frame,
    area: Rect,
) {
    let candidates = search.candidate_indices(groups);
    let n = candidates.len().min(6);
    let labels: Vec<String> = candidates
        .iter()
        .take(n)
        .map(|&i| groups[i].pattern.clone())
        .collect();
    // Color by each group's global enabled-pattern index for consistency
    // with strip pills; fall back to dim if disabled.
    let mut color_idx = 0usize;
    let mut group_color: Vec<Option<usize>> = Vec::with_capacity(groups.len());
    for g in groups {
        if g.enabled {
            group_color.push(Some(color_idx));
            color_idx += 1;
        } else {
            group_color.push(None);
        }
    }
    let styles: Vec<Style> = candidates
        .iter()
        .take(n)
        .map(|&i| match group_color[i] {
            Some(idx) => theme::highlight_style(idx),
            None => theme::disabled_chip_style(),
        })
        .collect();
    let selected = if n == 0 {
        0
    } else {
        search.selected.min(n - 1)
    };
    render_candidate_list("历史", &labels, &styles, selected, "无匹配历史", frame, area);
}

pub fn render_popup(input: &InputBox, frame: &mut Frame, area: Rect) {
    if !input.field_popup_visible() {
        return;
    }
    let matches = input.field_candidates();
    let labels: Vec<String> = matches.iter().map(|f| f.keyword().to_string()).collect();
    let styles: Vec<Style> = matches
        .iter()
        .map(|&f| Style::default().fg(theme::field_color(f)))
        .collect();
    let selected = if matches.is_empty() {
        0
    } else {
        input.field_selected.min(matches.len() - 1)
    };
    render_candidate_list("字段", &labels, &styles, selected, "无匹配字段", frame, area);
}

pub fn render_status_bar(app: &App, frame: &mut Frame, area: Rect) {
    let mut spans = vec![Span::styled(
        format!("{}/{}", app.cursor + 1, app.visible.len()),
        Style::default().add_modifier(Modifier::DIM),
    )];
    if let Some((current, total)) = app.search_match_stats() {
        let k = current.map(|n| n.to_string()).unwrap_or_else(|| "-".into());
        spans.push(Span::raw(" "));
        spans.push(Span::styled(
            format!("[{k}/{total}]"),
            theme::search_match_status_style(),
        ));
    }
    if app.following {
        spans.push(Span::raw(" "));
        spans.push(theme::status_badge("FOLLOWING", theme::success()));
    }
    if let Some(lock) = app.lock_badge_label() {
        spans.push(Span::raw(" "));
        spans.push(theme::status_badge(&lock, theme::lock()));
    }
    if app.visual_anchor.is_some() {
        spans.push(Span::raw(" "));
        spans.push(theme::status_badge("VISUAL", theme::accent()));
    } else if app.msg_chip_picker.is_some() {
        spans.push(Span::raw(" "));
        let label = if app.msg_chip_picker.as_ref().is_some_and(|p| p.as_exclude) {
            "C m…"
        } else {
            "c m…"
        };
        spans.push(theme::status_badge(label, theme::warning()));
    } else if app.pending_chip {
        spans.push(Span::raw(" "));
        spans.push(theme::status_badge("c…", theme::warning()));
    } else if app.pending_exclude {
        spans.push(Span::raw(" "));
        spans.push(theme::status_badge("C…", theme::warning()));
    } else if app.pending_lock {
        spans.push(Span::raw(" "));
        spans.push(theme::status_badge("f…", theme::warning()));
    } else if app.pending_yank {
        spans.push(Span::raw(" "));
        spans.push(theme::status_badge("y…", theme::warning()));
    }
    if let Some(msg) = &app.status_msg {
        if msg != "VISUAL"
            && msg != "y…"
            && msg != "c…"
            && msg != "c m…"
            && msg != "C…"
            && msg != "C m…"
            && msg != "f…"
        {
            spans.push(Span::raw(" "));
            let bg = if msg.starts_with("YANK FAILED") {
                theme::warning()
            } else {
                theme::accent()
            };
            spans.push(theme::status_badge(msg, bg));
        }
    }
    // Trailing context help (H6): badges keep priority; hint truncates or hides.
    let left_width: usize = spans.iter().map(span_width).sum();
    let avail = (area.width as usize)
        .saturating_sub(left_width)
        .saturating_sub(1); // leading space before the hint
    if let Some(help) = crate::help::fit_help(crate::help::context_help(app), avail) {
        spans.push(Span::raw(" "));
        spans.push(Span::styled(help.to_string(), theme::context_help_style()));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search_model::SearchGroup;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn cell_text(buf: &ratatui::buffer::Buffer) -> String {
        let mut s = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                s.push_str(buf[(x, y)].symbol());
            }
            s.push('\n');
        }
        s
    }

    #[test]
    fn test_render_log_list_shows_tag_and_msg() {
        let mut app = App::new(100);
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(crate::model::EntryRow::from_line("04-02 10:00:00.000  1  1 I MyTag   : hello world").unwrap()).unwrap();
        drop(tx);
        app.drain(&rx);

        let backend = TestBackend::new(80, 5);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render_log_list(&mut app, frame, frame.area()))
            .unwrap();

        let content = cell_text(terminal.backend().buffer());
        assert!(content.contains("MyTag"));
        assert!(content.contains("hello world"));
    }

    #[test]
    fn test_render_log_list_highlights_selected_row_with_soft_gray_when_focused() {
        let mut app = App::new(100);
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(crate::model::EntryRow::from_line("04-02 10:00:00.000  1  1 I RowOne  : first").unwrap()).unwrap();
        tx.send(crate::model::EntryRow::from_line("04-02 10:00:01.000  1  1 I RowTwo  : second").unwrap()).unwrap();
        drop(tx);
        app.drain(&rx);
        app.cursor = 1; // select the second row

        let backend = TestBackend::new(80, 5);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render_log_list(&mut app, frame, frame.area()))
            .unwrap();

        // Row 0 is the block's top border; content starts at y=1 (one row
        // per entry, no wrapping for these short messages) and x=1 (past
        // the left border column).
        let buf = terminal.backend().buffer();
        let selected_style = buf[(1, 2)].style();
        let unselected_style = buf[(1, 1)].style();
        assert_eq!(selected_style.bg, Some(Color::DarkGray), "focused selection must use the soft gray background");
        assert_ne!(unselected_style.bg, Some(Color::DarkGray), "unselected rows must not get the selection background");
        assert!(
            !selected_style.add_modifier.contains(Modifier::REVERSED),
            "must not use the old reverse-video style"
        );
    }

    #[test]
    fn test_render_log_list_no_highlight_when_log_list_unfocused() {
        let mut app = App::new(100);
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(crate::model::EntryRow::from_line("04-02 10:00:00.000  1  1 I RowOne  : first").unwrap()).unwrap();
        tx.send(crate::model::EntryRow::from_line("04-02 10:00:01.000  1  1 I RowTwo  : second").unwrap()).unwrap();
        drop(tx);
        app.drain(&rx);
        app.cursor = 1;
        app.focus = Focus::Input; // LogList no longer has keyboard focus

        let backend = TestBackend::new(80, 5);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render_log_list(&mut app, frame, frame.area()))
            .unwrap();

        let buf = terminal.backend().buffer();
        let selected_style = buf[(1, 2)].style();
        let unselected_style = buf[(1, 1)].style();
        assert_eq!(
            selected_style, unselected_style,
            "with LogList unfocused, the previously-selected row must look identical to any other row"
        );
    }

    #[test]
    fn test_selection_preserves_keyword_highlight_bg() {
        let mut app = App::new(100);
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(crate::model::EntryRow::from_line("04-02 10:00:00.000  1  1 I Tag     : an error here").unwrap())
            .unwrap();
        drop(tx);
        app.drain(&rx);
        app.search_groups
            .groups
            .push(SearchGroup::from_pattern("error").unwrap());
        app.focus = Focus::LogList;
        app.cursor = 0;

        let backend = TestBackend::new(80, 5);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render_log_list(&mut app, frame, frame.area()))
            .unwrap();

        let expected_hl = theme::highlight_style(0).bg;
        let buf = terminal.backend().buffer();
        // Scan the content row for a cell whose bg is the highlight color.
        let mut found = false;
        for x in 0..buf.area.width {
            if buf[(x, 1)].style().bg == expected_hl {
                found = true;
                break;
            }
        }
        assert!(found, "keyword highlight bg must survive selection overlay");
    }

    #[test]
    fn test_render_log_list_persists_scroll_offset_when_cursor_moves_within_viewport() {
        let mut app = App::new(100);
        let (tx, rx) = std::sync::mpsc::channel();
        for i in 0..30 {
            tx.send(crate::model::EntryRow::from_line(&format!("04-02 10:00:00.000  1  1 I Tag     : line{i}")).unwrap())
                .unwrap();
        }
        drop(tx);
        app.drain(&rx);
        app.following = false;

        // Backend height 7 minus 2 border rows leaves a 5-row inner viewport.
        let backend = TestBackend::new(80, 7);
        let mut terminal = Terminal::new(backend).unwrap();

        app.cursor = 20;
        terminal.draw(|frame| render_log_list(&mut app, frame, frame.area())).unwrap();
        let offset_at_edge = app.list_offset;
        assert!(offset_at_edge > 0, "cursor near the bottom of a 30-row list in a 5-row viewport must have scrolled");

        app.cursor -= 2; // moves, but stays inside the already-scrolled viewport
        terminal.draw(|frame| render_log_list(&mut app, frame, frame.area())).unwrap();

        assert_eq!(
            app.list_offset, offset_at_edge,
            "moving within the visible window must not re-scroll the viewport"
        );
    }

    #[test]
    fn test_wrap_ranges_breaks_on_whitespace() {
        let ranges = wrap_ranges("hello world foo", 11);
        let chunks: Vec<&str> = ranges.iter().map(|&(s, e)| &"hello world foo"[s..e]).collect();
        assert_eq!(chunks, vec!["hello world", "foo"]);
    }

    #[test]
    fn test_wrap_ranges_hard_cuts_overlong_word() {
        let text = "supercalifragilistic";
        let ranges = wrap_ranges(text, 6);
        assert!(ranges.len() > 1, "an overlong word must be split into multiple pieces");
        for &(s, e) in &ranges {
            assert!(e - s <= 6);
        }
        let rejoined: String = ranges.iter().map(|&(s, e)| &text[s..e]).collect();
        assert_eq!(rejoined, text);
    }

    #[test]
    fn test_wrap_ranges_short_text_is_single_range() {
        let ranges = wrap_ranges("short", 80);
        assert_eq!(ranges, vec![(0, 5)]);
    }

    #[test]
    fn test_render_entry_lines_wraps_long_message_into_multiple_lines() {
        let row = EntryRow::from_line(
            "04-02 10:00:00.000  1  1 I Tag     : this message is long enough that it must wrap across more than one physical line when the column width is narrow",
        )
        .unwrap();
        let lines = render_entry_lines(&row, &[], 40, 1, 1);
        assert!(lines.len() > 1, "a long message should wrap into multiple lines, got {}", lines.len());
    }

    #[test]
    fn test_render_entry_lines_highlights_only_matched_keyword() {
        let row = EntryRow::from_line("04-02 10:00:00.000  1  1 I Tag     : an error occurred here").unwrap();
        let re = Regex::new("(?i)error").unwrap();
        let patterns = [(&re, 0usize, false)];
        let lines = render_entry_lines(&row, &patterns, 200, 1, 1);
        assert_eq!(lines.len(), 1);
        let matched: Vec<&Span> = lines[0].spans.iter().filter(|s| s.content.as_ref() == "error").collect();
        assert_eq!(matched.len(), 1, "exactly the matched keyword should be its own span");
        let other_span_styles: Vec<Style> =
            lines[0].spans.iter().filter(|s| s.content.as_ref() != "error").map(|s| s.style).collect();
        assert!(
            other_span_styles.iter().all(|s| *s != matched[0].style),
            "non-matched spans must not share the highlight style"
        );
    }

    #[test]
    fn test_render_entry_lines_highlights_tag_matches() {
        let row = EntryRow::from_line("04-02 10:00:00.000  1  1 I MyTag   : hello world").unwrap();
        let re = Regex::new("(?i)tag").unwrap();
        let patterns = [(&re, 0usize, false)];
        let lines = render_entry_lines(&row, &patterns, 200, 1, 1);
        let matched = lines[0]
            .spans
            .iter()
            .find(|s| s.content.as_ref() == "Tag")
            .expect("tag substring should be its own highlighted span");
        assert_eq!(matched.style, theme::highlight_style(0));
        let prefix = lines[0]
            .spans
            .iter()
            .find(|s| s.content.as_ref() == "My")
            .expect("unmatched tag prefix keeps accent style");
        assert_eq!(
            prefix.style,
            Style::default().fg(theme::accent()).add_modifier(Modifier::BOLD)
        );
    }

    #[test]
    fn test_render_entry_lines_multicolor_patterns() {
        let row = EntryRow::from_line("04-02 10:00:00.000  1  1 I Tag     : foo and bar here").unwrap();
        let re0 = Regex::new("(?i)foo").unwrap();
        let re1 = Regex::new("(?i)bar").unwrap();
        let patterns = [(&re0, 0usize, true), (&re1, 1usize, false)];
        let lines = render_entry_lines(&row, &patterns, 200, 1, 1);
        let foo = lines[0].spans.iter().find(|s| s.content.as_ref() == "foo").unwrap();
        let bar = lines[0].spans.iter().find(|s| s.content.as_ref() == "bar").unwrap();
        assert_eq!(foo.style, theme::highlight_style_active(0));
        assert_eq!(bar.style, theme::highlight_style(1));
        assert!(foo.style.add_modifier.contains(Modifier::UNDERLINED));
        assert!(!bar.style.add_modifier.contains(Modifier::UNDERLINED));
        assert_ne!(foo.style.bg, bar.style.bg);
    }

    #[test]
    fn test_render_entry_lines_uses_natural_tag_width_no_fixed_padding() {
        let row = EntryRow::from_line("04-02 10:00:00.000  1  1 I Ab   : msg").unwrap();
        let lines = render_entry_lines(&row, &[], 200, 1, 1);
        let tag_span = lines[0]
            .spans
            .iter()
            .find(|s| s.content.as_ref().starts_with("Ab"))
            .expect("tag span");
        assert_eq!(tag_span.content.as_ref(), "Ab ", "short tag must not be padded to 16 columns");
    }

    #[test]
    fn test_render_entry_lines_continuation_indent_matches_header_width() {
        let row = EntryRow::from_line(
            "04-02 10:00:00.000  1  1 I Short : this message is long enough that it must wrap across more than one physical line when the column width is narrow",
        )
        .unwrap();
        let lines = render_entry_lines(&row, &[], 40, 1, 1);
        assert!(lines.len() > 1);
        // lineno + timestamp + level + tag
        let header_width: usize = lines[0].spans.iter().take(4).map(|s| s.content.chars().count()).sum();
        let cont = lines[1].spans[0].content.as_ref();
        assert!(cont.chars().all(|c| c == ' '), "continuation prefix should be spaces");
        assert_eq!(cont.chars().count(), header_width);
    }

    #[test]
    fn test_render_entry_lines_shows_lineno_without_pid_tid() {
        let row = EntryRow::from_line("04-02 10:00:00.000  1234  5678 I MyTag   : hello").unwrap();
        let lines = render_entry_lines(&row, &[], 200, 12, 3);
        let text: String = lines[0].spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains(" 12 "), "lineno should be right-aligned to width 3");
        assert!(text.contains("MyTag"));
        assert!(text.contains("hello"));
        assert!(!text.contains("1234"), "pid must not appear in default display");
        assert!(!text.contains("5678"), "tid must not appear in default display");
        assert!(text.contains(" I "), "level badge must remain");
    }

    #[test]
    fn test_chip_pill_and_search_pill_styles() {
        let (text, body) = theme::chip_pill_style(crate::input::ChipField::Tag, "MyTag", false);
        assert!(text.contains("MyTag"));
        assert_eq!(body.bg, Some(theme::accent()));
        let (_, disabled) = theme::chip_pill_style(crate::input::ChipField::Msg, "x", true);
        assert_eq!(disabled, theme::disabled_chip_style());
        let (_, search) = theme::search_pill_style("error", 0, false, false);
        assert_eq!(search, theme::highlight_style(0));
        let (_, active) = theme::search_pill_style("error", 0, false, true);
        assert_eq!(active, theme::highlight_style_active(0));
        assert!(active.add_modifier.contains(Modifier::UNDERLINED));
    }

    #[test]
    fn test_render_input_box_shows_caret_after_committed_pill() {
        use crate::input::{Chip, ChipField};

        let mut input = InputBox::default();
        input.chips.push(Chip {
            field: ChipField::Tag,
            value: "MyTag".into(),
        });
        // Continue typing after the pill — the historical bug skipped caret
        // entirely once chips were non-empty.
        input.draft = "x".into();

        let backend = TestBackend::new(40, 3);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render_input_box(&input, Mode::Insert, true, frame, frame.area()))
            .unwrap();
        let buf = terminal.backend().buffer();
        let content = cell_text(buf);
        assert!(
            content.contains('▏'),
            "Insert caret must remain visible after a committed pill, got: {content:?}"
        );
        assert!(content.contains("MyTag"));
        assert!(content.contains('x'));

        // Caret cell must not inherit the pill's filled background.
        let mut found_caret = false;
        for x in 0..buf.area.width {
            let cell = &buf[(x, 1)];
            if cell.symbol() == "▏" {
                found_caret = true;
                assert_eq!(cell.fg, theme::accent());
                assert_ne!(
                    cell.bg,
                    theme::accent(),
                    "caret must not sit on the Tag pill's cyan fill"
                );
            }
        }
        assert!(found_caret, "caret glyph missing from content row");
    }

    #[test]
    fn test_chip_strip_selection_keeps_stable_layout() {
        use crate::filter_model::Group;
        use crate::input::{Chip, ChipField};

        let mut app = App::new(100);
        app.groups.groups.push(Group {
            label: "a".into(),
            chips: vec![Chip { field: ChipField::Tag, value: "A".into() }],
            expr: None,
            time: None,
            enabled: true,
        });
        app.groups.groups.push(Group {
            label: "b".into(),
            chips: vec![Chip { field: ChipField::Msg, value: "B".into() }],
            expr: None,
            time: None,
            enabled: true,
        });
        app.focus = Focus::ChipStrip;
        app.group_cursor = 0;

        // Single content row + outer rounded chrome = height 3.
        let backend = TestBackend::new(60, 3);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render_chip_strip(&app, frame, frame.area()))
            .unwrap();
        let before = cell_text(terminal.backend().buffer());

        app.group_cursor = 1;
        terminal
            .draw(|frame| render_chip_strip(&app, frame, frame.area()))
            .unwrap();
        let after = cell_text(terminal.backend().buffer());

        // Selection only restyles the ● — glyph layout stays put.
        assert!(before.contains('A') && after.contains('A'));
        assert!(before.contains('B') && after.contains('B'));
        assert_eq!(
            before.chars().filter(|c| *c == 'A' || *c == 'B').count(),
            after.chars().filter(|c| *c == 'A' || *c == 'B').count()
        );
        let corners = before.chars().filter(|c| matches!(*c, '╭' | '╮' | '╰' | '╯')).count();
        assert_eq!(corners, 4, "only strip outer rounded chrome, got {corners}");
    }

    #[test]
    fn test_filter_strip_wraps_and_grows_height() {
        use crate::filter_model::Group;
        use crate::input::{Chip, ChipField};

        let mut app = App::new(100);
        for label in ["AAAA", "BBBB", "CCCC", "DDDD"] {
            app.groups.groups.push(Group {
                label: label.into(),
                chips: vec![Chip {
                    field: ChipField::Tag,
                    value: label.into(),
                }],
                expr: None,
                time: None,
                enabled: true,
            });
        }
        let h = filter_strip_height(&app, 20);
        assert!(h > 3, "wrapped strip should exceed one content row + chrome, got {h}");
        assert_eq!(filter_strip_height(&app, 20), h, "height is instantaneous (stable)");
        app.groups.groups.clear();
        assert_eq!(filter_strip_height(&app, 20), 0);
    }

    #[test]
    fn test_render_status_bar_shows_search_match_stats() {
        let mut app = App::new(100);
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(crate::model::EntryRow::from_line("04-02 10:00:00.000  1  1 I T   : aaa").unwrap()).unwrap();
        tx.send(crate::model::EntryRow::from_line("04-02 10:00:01.000  1  1 I T   : hit one").unwrap()).unwrap();
        tx.send(crate::model::EntryRow::from_line("04-02 10:00:02.000  1  1 I T   : hit two").unwrap()).unwrap();
        drop(tx);
        app.drain(&rx);
        app.following = false;
        app.cursor = 0;
        app.push_or_find_search_group(SearchGroup::from_pattern("hit").unwrap());

        let backend = TestBackend::new(40, 1);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render_status_bar(&app, frame, frame.area()))
            .unwrap();
        let content = cell_text(terminal.backend().buffer());
        assert!(content.contains("[-/2]"), "cursor not on hit: got {content:?}");

        app.cursor = 1;
        terminal
            .draw(|frame| render_status_bar(&app, frame, frame.area()))
            .unwrap();
        let content = cell_text(terminal.backend().buffer());
        assert!(content.contains("[1/2]"), "first hit ordinal: got {content:?}");
    }

    #[test]
    fn test_render_status_bar_shows_context_help_when_wide() {
        let mut app = App::new(100);
        app.following = false;
        app.focus = Focus::LogList;

        let backend = TestBackend::new(80, 1);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render_status_bar(&app, frame, frame.area()))
            .unwrap();
        let content = cell_text(terminal.backend().buffer());
        assert!(
            content.contains("j/k") && content.contains("Esc"),
            "wide bar should show LogList help: got {content:?}"
        );
    }

    #[test]
    fn test_render_status_bar_hides_context_help_when_narrow() {
        let mut app = App::new(100);
        app.following = true; // FOLLOWING badge consumes space
        app.focus = Focus::LogList;

        // Wide enough for "1/0" + FOLLOWING badge, too tight for help (avail < 8).
        let backend = TestBackend::new(20, 1);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render_status_bar(&app, frame, frame.area()))
            .unwrap();
        let content = cell_text(terminal.backend().buffer());
        assert!(
            content.contains("FOLLOWING"),
            "badges must win over help: got {content:?}"
        );
        assert!(
            !content.contains("下一命中") && !content.contains("j/k"),
            "narrow bar should hide help entirely: got {content:?}"
        );
    }

    #[test]
    fn test_minimap_row_maps_ends() {
        assert_eq!(minimap_row_for_index(0, 100, 10), 0);
        assert_eq!(minimap_row_for_index(99, 100, 10), 9);
        assert_eq!(minimap_row_for_index(50, 100, 10), 4);
    }

    #[test]
    fn test_build_minimap_marks_severe_and_search() {
        let mut app = App::new(100);
        let (tx, rx) = std::sync::mpsc::channel();
        for (i, level, msg) in [
            (0, "I", "ok"),
            (1, "E", "boom"),
            (2, "I", "findme here"),
            (3, "I", "ok"),
        ] {
            let _ = i;
            tx.send(
                EntryRow::from_line(&format!(
                    "04-02 10:00:00.000  1  1 {level} Tag     : {msg}"
                ))
                .unwrap(),
            )
            .unwrap();
        }
        drop(tx);
        app.drain(&rx);
        app.search_groups
            .groups
            .push(SearchGroup::from_pattern("findme").unwrap());
        app.list_offset = 0;

        let marks = build_minimap_marks(&app, 4);
        assert_eq!(marks.len(), 4);
        assert!(marks.iter().any(|m| *m == MinimapMark::Severe));
        assert!(marks.iter().any(|m| *m == MinimapMark::Search));
        assert!(marks.iter().any(|m| *m == MinimapMark::Viewport));
        // Index 1 (E) maps near row 1 of 4.
        assert_eq!(
            marks[minimap_row_for_index(1, 4, 4)],
            MinimapMark::Severe
        );
    }

    #[test]
    fn test_build_minimap_empty_when_no_visible() {
        let app = App::new(100);
        assert!(build_minimap_marks(&app, 10).is_empty());
    }

    #[test]
    fn test_render_log_list_draws_minimap_rail() {
        let mut app = App::new(100);
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(EntryRow::from_line("04-02 10:00:00.000  1  1 E Tag     : err").unwrap())
            .unwrap();
        for i in 0..8 {
            tx.send(
                EntryRow::from_line(&format!("04-02 10:00:00.000  1  1 I Tag     : line{i}"))
                    .unwrap(),
            )
            .unwrap();
        }
        drop(tx);
        app.drain(&rx);

        let backend = TestBackend::new(40, 8);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render_log_list(&mut app, frame, frame.area()))
            .unwrap();
        let buf = terminal.backend().buffer();
        // Inner rightmost content col (width 40 → border at 0/39, inner 1..38, rail at 38).
        let rail_x = buf.area.width - 2;
        let mut found_mark = false;
        for y in 1..buf.area.height.saturating_sub(1) {
            let ch = buf[(rail_x, y)].symbol();
            if ch == "•" || ch == "│" {
                found_mark = true;
                break;
            }
        }
        assert!(found_mark, "minimap rail should paint │/• inside the log border");
    }
}
