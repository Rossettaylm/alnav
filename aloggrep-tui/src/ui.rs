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
        .highlight_style(theme::focus_style())
        .highlight_symbol("\u{203a} ");
    let mut state = ListState::default();
    state.select(Some(selected.min(labels.len() - 1)));
    frame.render_stateful_widget(list, area, &mut state);
}

/// Field-candidate popup height: `clamp(count,1,6)+2` for border, clamped to
/// space above the Input modal anchor.
pub fn candidate_popup_rect(anchor: Rect, frame: Rect, match_count: usize) -> Rect {
    let desired = match_count.clamp(1, 6) as u16 + 2;
    let space_above = anchor.y.saturating_sub(frame.y);
    let height = desired.min(space_above).max(1);
    Rect {
        x: anchor.x,
        y: anchor.y.saturating_sub(height),
        width: anchor.width,
        height,
    }
}

/// Search modal outer height: input row (+borders) plus optional candidate rows.
pub fn search_modal_height(candidate_count: usize) -> u16 {
    let input_shell = 3u16; // border + 1 content line
    if candidate_count == 0 {
        input_shell
    } else {
        input_shell + candidate_count.clamp(1, 6) as u16
    }
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

/// Splits `msg[range.0..range.1]` into plain/highlighted spans.
fn spans_for_range(msg: &str, range: (usize, usize), matches: &[ColoredMatch]) -> Vec<Span<'static>> {
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
            spans.push(Span::raw(msg[cursor..seg_start].to_string()));
        }
        let style = if is_active {
            theme::highlight_style_active(color_idx)
        } else {
            theme::highlight_style(color_idx)
        };
        spans.push(Span::styled(msg[seg_start..seg_end].to_string(), style));
        cursor = seg_end;
    }
    if cursor < end {
        spans.push(Span::raw(msg[cursor..end].to_string()));
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
    let tag = format!("{} ", row.tag);
    let header_width =
        lineno_s.chars().count() + ts.chars().count() + level_badge.chars().count() + tag.chars().count();
    let cont_prefix: String = " ".repeat(header_width);

    let first_width = area_width.saturating_sub(header_width).max(8);
    let cont_width = area_width.saturating_sub(header_width).max(8);

    let matches = collect_matches(&row.msg, patterns);

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
                spans.push(Span::styled(tag.clone(), Style::default().fg(theme::ACCENT).add_modifier(Modifier::BOLD)));
            } else {
                spans.push(Span::styled(cont_prefix.clone(), Style::default().add_modifier(Modifier::DIM)));
            }
            spans.extend(spans_for_range(&row.msg, range, &matches));
            Line::from(spans)
        })
        .collect()
}

/// Takes `&mut App` (unlike sibling `render_*` functions) so ratatui's
/// scroll offset can be persisted across frames via `App.list_offset` —
/// do not revert this to `&App`, that's exactly what caused the old
/// viewport-snap bug.
pub fn render_log_list(app: &mut App, frame: &mut Frame, area: Rect) {
    let active = app.focus == Focus::LogList;
    let block = rounded_block(theme::numbered_title(3, "Log", active), active);
    let inner_width = block.inner(area).width.max(1) as usize;
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
    // No List::highlight_style — selection is painted on the item above.
    let list = List::new(items).block(block);
    let mut state = ListState::default().with_offset(app.list_offset);
    if !app.visible.is_empty() {
        state.select(Some(app.cursor));
    }
    frame.render_stateful_widget(list, area, &mut state);
    app.list_offset = state.offset();
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

pub fn render_search_chip_strip(app: &App, frame: &mut Frame, area: Rect) {
    if area.height == 0 {
        return;
    }
    let active = app.focus == Focus::SearchStrip;
    let block = rounded_block(theme::numbered_title(2, "Search", active), active);
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
        let (text, body) = theme::chip_pill_style(chip.field, &chip.value, false);
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
    let inner = render_modal_shell("Input", frame, area);
    frame.render_widget(
        Paragraph::new(Line::from(input_content_spans(input, mode == Mode::Insert))),
        inner,
    );
}

/// Legacy single-row Input render kept for unit tests that draw into a fixed area.
pub fn render_input_box(input: &InputBox, mode: Mode, focused: bool, frame: &mut Frame, area: Rect) {
    let block = rounded_block(theme::numbered_title(4, "Input", focused), focused);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(Clear, inner);
    frame.render_widget(
        Paragraph::new(Line::from(input_content_spans(input, mode == Mode::Insert))),
        inner,
    );
}

/// Centered Search modal with optional history-chip candidate rows below the draft.
pub fn render_search_modal(
    search: &SearchBox,
    groups: &[SearchGroup],
    frame: &mut Frame,
    area: Rect,
) {
    let candidates = search.candidate_indices(groups);
    let n = candidates.len().min(6);
    frame.render_widget(Clear, area);

    let block = rounded_block(theme::plain_title("Search", true), true);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let [draft_area, list_area] = if n == 0 {
        [inner, Rect::default()]
    } else {
        let [d, l] = Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).areas(inner);
        [d, l]
    };

    frame.render_widget(Clear, draft_area);
    let spans = vec![
        Span::styled(
            "/",
            Style::reset().fg(theme::ACCENT).add_modifier(Modifier::BOLD),
        ),
        Span::styled(search.draft.clone(), Style::reset()),
        theme::caret_bar(),
    ];
    frame.render_widget(Paragraph::new(Line::from(spans)), draft_area);

    if n > 0 && list_area.height > 0 {
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
        // Render list without an extra outer title (already inside Search shell).
        let items: Vec<ListItem> = labels
            .iter()
            .zip(styles.iter())
            .map(|(label, style)| ListItem::new(Span::styled(format!(" {label} "), *style)))
            .collect();
        let list = List::new(items)
            .highlight_style(theme::focus_style())
            .highlight_symbol("\u{203a} ");
        let mut state = ListState::default();
        state.select(Some(search.selected.min(n - 1)));
        frame.render_stateful_widget(list, list_area, &mut state);
    }
}

pub fn render_popup(input: &InputBox, frame: &mut Frame, area: Rect) {
    let Some(popup) = &input.popup else { return };
    let matches = popup.matches();
    let labels: Vec<String> = matches.iter().map(|f| f.keyword().to_string()).collect();
    let styles: Vec<Style> = matches
        .iter()
        .map(|&f| Style::default().fg(theme::field_color(f)))
        .collect();
    render_candidate_list("字段", &labels, &styles, popup.selected, "无匹配字段", frame, area);
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
        spans.push(theme::status_badge("FOLLOWING", theme::SUCCESS));
    }
    if app.visual_anchor.is_some() {
        spans.push(Span::raw(" "));
        spans.push(theme::status_badge("VISUAL", theme::ACCENT));
    } else if app.pending_yank {
        spans.push(Span::raw(" "));
        spans.push(theme::status_badge("y…", theme::WARNING));
    }
    if let Some(msg) = &app.status_msg {
        if msg != "VISUAL" && msg != "y…" {
            spans.push(Span::raw(" "));
            let bg = if msg.starts_with("YANK FAILED") {
                theme::WARNING
            } else {
                theme::ACCENT
            };
            spans.push(theme::status_badge(msg, bg));
        }
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
        assert_eq!(body.bg, Some(theme::ACCENT));
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
                assert_eq!(cell.fg, theme::ACCENT);
                assert_ne!(
                    cell.bg,
                    theme::ACCENT,
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
}
