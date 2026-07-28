use ratatui::layout::{Alignment, Constraint, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph};
use ratatui::Frame;
use regex::Regex;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::app::{App, Focus, Mode};
use crate::filter_model::Group;
use crate::highlight_model::{HighlightBox, HighlightGroup};
use crate::input::{ChipField, InputBox};
use crate::model::EntryRow;
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
/// fzf picker outer frame: leave 2 cols margin each side.
const PICKER_FRAME_WIDTH_MARGIN: u16 = 4;
/// fzf picker height ≈ 75% of frame, clamped to this minimum.
const PICKER_FRAME_MIN_HEIGHT: u16 = 10;
/// Minimum width for each left/right pane inside the picker.
const PICKER_LR_MIN_WIDTH: u16 = 10;
/// Rounded search input height at the bottom of the left pane.
const PICKER_SEARCH_HEIGHT: u16 = 3;
/// Horizontal padding between the search border and its content.
const PICKER_SEARCH_HORIZONTAL_PADDING: u16 = 1;
/// Gap between adjacent popup surfaces (Picker L/R, modal → candidates → Preview).
const POPUP_GAP: u16 = 1;
/// LogList tag column width (display columns); short tags pad, long tags truncate.
const TAG_COL_WIDTH: usize = 20;
/// Floor for the tag column when the pane is narrow (still may shrink further).
const TAG_COL_MIN: usize = 4;
/// Gap between level badge and tag column (outside badge fill).
const LEVEL_TAG_GAP: usize = 1;
/// Gap between the fixed tag column and the message.
const TAG_MSG_GAP: usize = 1;
/// Per-row action icon for candidate lists (F3). `None` = no icon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionKind {
    None,
    Jump,
    Toggle { enabled: bool },
}

impl ActionKind {
    fn icon(self) -> &'static str {
        match self {
            ActionKind::None => "",
            ActionKind::Jump => theme::GLYPH_ACTION_JUMP,
            ActionKind::Toggle { enabled: true } => theme::GLYPH_ACTION_TOGGLE_ON,
            ActionKind::Toggle { enabled: false } => theme::GLYPH_ACTION_TOGGLE_OFF,
        }
    }

    fn icon_style(self) -> Style {
        match self {
            ActionKind::None => Style::default(),
            ActionKind::Jump => Style::default().fg(theme::accent()),
            ActionKind::Toggle { enabled: true } => Style::default().fg(theme::success()),
            ActionKind::Toggle { enabled: false } => theme::disabled_chip_style(),
        }
    }
}

fn rounded_block(title: Line<'static>, active: bool) -> Block<'static> {
    Block::new()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(theme::border_style(active))
        .title(title)
}

/// Top/bottom-only divider block (Q3 path B: weakened borders). Uses
/// box-drawing `─` (U+2500) for horizontal rules; no left/right borders,
/// giving the inner content 2 extra columns vs `rounded_block`.
fn divider_block(title: Line<'static>, active: bool) -> Block<'static> {
    Block::new()
        .borders(Borders::TOP | Borders::BOTTOM)
        .border_type(BorderType::Plain)
        .border_style(theme::border_style(active))
        .title(title)
}

/// Unified width for centered Input / Search modals.
pub fn modal_width(frame_width: u16) -> u16 {
    frame_width
        .saturating_sub(4)
        .clamp(MODAL_WIDTH_MIN, MODAL_WIDTH_MAX)
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
    let y = frame
        .y
        .saturating_add(1)
        .min(frame.y.saturating_add(frame.height.saturating_sub(height)));
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

/// Like [`stack_below_rect`], but leave [`POPUP_GAP`] rows when there is room;
/// if the remaining space is ≤ gap, pack flush (no gap) so a 1-row sliver still fits.
pub fn stack_below_rect_gapped(anchor: Rect, frame: Rect, height: u16) -> Rect {
    let flush_y = anchor.y.saturating_add(anchor.height);
    let frame_bottom = frame.y.saturating_add(frame.height);
    let space_flush = frame_bottom.saturating_sub(flush_y);
    if space_flush > POPUP_GAP {
        let gapped_anchor = Rect {
            x: anchor.x,
            y: anchor.y,
            width: anchor.width,
            height: anchor.height.saturating_add(POPUP_GAP),
        };
        stack_below_rect(gapped_anchor, frame, height)
    } else {
        stack_below_rect(anchor, frame, height)
    }
}

/// Horizontal center, height ≈ 75% of `frame` (clamped to a readable minimum).
/// When `show_preview` is false, width is ≈ half of the full picker width.
pub fn picker_frame_rect(frame: Rect, show_preview: bool) -> Rect {
    let full_w = frame
        .width
        .saturating_sub(PICKER_FRAME_WIDTH_MARGIN)
        .max(PICKER_LR_MIN_WIDTH.saturating_mul(2));
    let width = if show_preview {
        full_w
    } else {
        (full_w / 2).max(PICKER_LR_MIN_WIDTH)
    };
    let height = (frame.height * 3 / 4)
        .max(PICKER_FRAME_MIN_HEIGHT)
        .min(frame.height);
    let x = frame.x + (frame.width.saturating_sub(width)) / 2;
    let y = frame.y + (frame.height.saturating_sub(height)) / 2;
    Rect {
        x,
        y,
        width,
        height,
    }
}

/// Split `area` into left/right panes by `left_ratio`; each pane is at least
/// [`PICKER_LR_MIN_WIDTH`] columns wide.
pub fn split_picker_lr(area: Rect, left_ratio: f32) -> (Rect, Rect) {
    let total = area.width;
    let mut left_w = ((total as f32) * left_ratio).round() as u16;
    left_w = left_w
        .max(PICKER_LR_MIN_WIDTH)
        .min(total.saturating_sub(PICKER_LR_MIN_WIDTH));
    let right_w = total.saturating_sub(left_w);
    let left = Rect {
        x: area.x,
        y: area.y,
        width: left_w,
        height: area.height,
    };
    let right = Rect {
        x: area.x + left_w,
        y: area.y,
        width: right_w,
        height: area.height,
    };
    (left, right)
}

/// Like [`split_picker_lr`], but leave [`POPUP_GAP`] columns between panes.
pub fn split_picker_lr_gapped(area: Rect, left_ratio: f32) -> (Rect, Rect) {
    let gap = POPUP_GAP.min(
        area.width
            .saturating_sub(PICKER_LR_MIN_WIDTH.saturating_mul(2)),
    );
    let usable = Rect {
        x: area.x,
        y: area.y,
        width: area.width.saturating_sub(gap),
        height: area.height,
    };
    let (left, right_inner) = split_picker_lr(usable, left_ratio);
    let right = Rect {
        x: left.x.saturating_add(left.width).saturating_add(gap),
        y: area.y,
        width: right_inner.width,
        height: area.height,
    };
    (left, right)
}

/// Left pane vertical stack: candidates fill the top, search area pinned to bottom.
pub fn picker_left_stack(left: Rect, has_chips: bool) -> (Rect, Rect) {
    let chip_h = if has_chips { 1 } else { 0 };
    let search_h = PICKER_SEARCH_HEIGHT.saturating_add(chip_h).min(left.height);
    let cand_h = left.height.saturating_sub(search_h);
    let candidates = Rect {
        x: left.x,
        y: left.y,
        width: left.width,
        height: cand_h,
    };
    let search = Rect {
        x: left.x,
        y: left.y + cand_h,
        width: left.width,
        height: search_h,
    };
    (candidates, search)
}

/// Rounded four-sided popup shell with a glyph-prefixed plain title.
/// Returns the inner content rect.
fn popup_block(title: &str) -> Block<'static> {
    rounded_block(
        theme::plain_title(theme::GLYPH_TITLE_PICKER, title, true),
        true,
    )
}

/// Clear + rounded full-border shell (dim accent). Returns the inner content rect.
pub fn render_modal_shell(title: &str, frame: &mut Frame, area: Rect) -> Rect {
    frame.render_widget(Clear, area);
    let block = popup_block(title);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(Clear, inner);
    inner
}

/// Find ignore-case substring match as byte range in `haystack`, or `None`.
pub fn find_ignore_case_range(haystack: &str, needle: &str) -> Option<(usize, usize)> {
    if needle.is_empty() {
        return None;
    }
    let hay: Vec<(usize, char)> = haystack.char_indices().collect();
    let needle_lower = needle.to_lowercase();
    let needle_len = needle.chars().count();
    if needle_len == 0 || hay.len() < needle_len {
        return None;
    }
    for start in 0..=hay.len() - needle_len {
        let window: String = hay[start..start + needle_len]
            .iter()
            .map(|(_, c)| *c)
            .collect::<String>()
            .to_lowercase();
        if window == needle_lower {
            let byte_start = hay[start].0;
            let byte_end = hay
                .get(start + needle_len)
                .map(|(i, _)| *i)
                .unwrap_or(haystack.len());
            return Some((byte_start, byte_end));
        }
    }
    None
}

/// Build label spans with optional substring match coloring.
/// `checked` changes the selection-marker (prefix) color for Tab multi-select.
fn candidate_label_spans(
    label: &str,
    query: &str,
    selected: bool,
    checked: bool,
    base: Style,
    action: ActionKind,
    area_width: u16,
) -> Vec<Span<'static>> {
    use crate::bookmark::fit_label;
    let match_style = theme::candidate_match_style(selected);
    let prefix = if selected || checked {
        theme::candidate_prefix()
    } else {
        " ".repeat(theme::candidate_prefix().chars().count().max(1))
    };
    let prefix_style = if checked {
        theme::candidate_checked_prefix_style().bg(base.bg.unwrap_or(Color::Reset))
    } else {
        base
    };
    // icon occupies 1 glyph + 1 trailing pad when present.
    let icon_glyph = action.icon();
    let icon_w: u16 = if icon_glyph.is_empty() { 0 } else { 2 };
    let prefix_len = prefix.chars().count() as u16;
    // label budget = area − prefix − icon+pad − 1 trailing pad
    let label_max = (area_width as usize)
        .saturating_sub(prefix_len as usize)
        .saturating_sub(icon_w as usize)
        .saturating_sub(1)
        .max(1);
    let truncated = fit_label(label, label_max);
    let mut spans = vec![Span::styled(prefix, prefix_style)];
    if let Some((s, e)) = find_ignore_case_range(&truncated, query) {
        if s > 0 {
            spans.push(Span::styled(truncated[..s].to_string(), base));
        }
        spans.push(Span::styled(truncated[s..e].to_string(), match_style));
        if e < truncated.len() {
            spans.push(Span::styled(truncated[e..].to_string(), base));
        }
    } else {
        spans.push(Span::styled(truncated, base));
    }
    // padding to push the icon flush right, then the icon span.
    let used: usize = spans.iter().map(|sp| sp.content.chars().count()).sum();
    let pad = (area_width as usize)
        .saturating_sub(used)
        .saturating_sub(icon_w as usize);
    if pad > 0 {
        spans.push(Span::raw(" ".repeat(pad)));
    }
    if !icon_glyph.is_empty() {
        spans.push(Span::styled(icon_glyph.to_string(), action.icon_style()));
    }
    spans
}

/// Candidate list skin shared by field popup and Highlight history completion.
/// Selection / match colors and selected-row prefix come from [`theme`].
/// `checked` (same length as `labels`, or empty) marks Tab multi-select rows.
/// When `bordered` is true, draws a rounded popup shell (standalone field/history
/// popups); when false, fills `area` with no chrome (Picker left pane already
/// has an outer shell).
pub fn render_candidate_list(
    title: &str,
    labels: &[String],
    styles: &[Style],
    checked: &[bool],
    actions: &[ActionKind],
    selected: usize,
    empty_msg: &str,
    query: &str,
    frame: &mut Frame,
    area: Rect,
    bordered: bool,
) {
    let inner = if bordered {
        frame.render_widget(Clear, area);
        let block = popup_block(title);
        let inner = block.inner(area);
        frame.render_widget(block, area);
        inner
    } else {
        area
    };
    if labels.is_empty() {
        frame.render_widget(
            Paragraph::new(Span::styled(
                empty_msg,
                Style::default().add_modifier(Modifier::DIM),
            )),
            inner,
        );
        return;
    }
    let sel = selected.min(labels.len() - 1);
    let items: Vec<ListItem> = labels
        .iter()
        .enumerate()
        .map(|(i, label)| {
            let is_sel = i == sel;
            let is_checked = checked.get(i).copied().unwrap_or(false);
            let mut base = if is_sel {
                theme::candidate_selected_style()
            } else {
                theme::candidate_unselected_style()
            };
            // Kind/field-colored candidates keep their fg when not selected.
            if !is_sel {
                if let Some(style) = styles.get(i) {
                    if let Some(fg) = style.fg {
                        base = base.fg(fg);
                    }
                }
            }
            ListItem::new(Line::from(candidate_label_spans(
                label,
                query,
                is_sel,
                is_checked,
                base,
                actions.get(i).copied().unwrap_or(ActionKind::None),
                inner.width,
            )))
            .style(base)
        })
        .collect();
    let list = List::new(items)
        .highlight_style(Style::default())
        .highlight_symbol("");
    let mut state = ListState::default();
    state.select(Some(sel));
    frame.render_stateful_widget(list, inner, &mut state);
}

/// Candidate popup height: `clamp(count,1,8)+2` for border, clamped to
/// space below the modal anchor (Input / Search / H7 msg share this).
pub fn candidate_popup_rect(anchor: Rect, frame: Rect, match_count: usize) -> Rect {
    let desired = match_count.clamp(1, 8) as u16 + 2;
    stack_below_rect_gapped(anchor, frame, desired)
}

/// H1 Preview window height: content rows + border, clamped to space below
/// the previous stack item (candidates or modal).
pub fn preview_popup_rect(anchor: Rect, frame: Rect, content_rows: usize) -> Rect {
    let desired = (content_rows.clamp(1, 12) as u16).saturating_add(2);
    stack_below_rect_gapped(anchor, frame, desired)
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
            let char_offsets: Vec<usize> = text[ws..we]
                .char_indices()
                .map(|(i, _)| ws + i)
                .chain(std::iter::once(we))
                .collect();
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
    if patterns.is_empty() || msg.is_empty() {
        return Vec::new();
    }
    // Build a compact sorted list of non-overlapping match intervals.
    // Each successive pattern's matches overwrite earlier patterns on overlap
    // ("later pattern wins") — same semantics as the original per-byte approach
    // but without allocating vec![None; msg.len()] for every call.
    let mut result: Vec<ColoredMatch> = Vec::new();

    for &(re, color_idx, is_active) in patterns {
        for m in re.find_iter(msg) {
            let ns = m.start();
            let ne = m.end();
            if ns >= ne {
                continue;
            }
            // Clip or remove existing intervals that overlap [ns, ne).
            let mut tmp = Vec::with_capacity(result.len() + 2);
            for (es, ee, ec, ea) in result.drain(..) {
                if ee <= ns || es >= ne {
                    tmp.push((es, ee, ec, ea)); // no overlap: keep as-is
                } else {
                    if es < ns {
                        tmp.push((es, ns, ec, ea)); // left remnant before new interval
                    }
                    if ee > ne {
                        tmp.push((ne, ee, ec, ea)); // right remnant after new interval
                    }
                    // the middle [max(es,ns), min(ee,ne)] is overwritten — drop
                }
            }
            tmp.push((ns, ne, color_idx, is_active));
            tmp.sort_unstable_by_key(|&(s, _, _, _)| s);
            result = tmp;
        }
    }

    result
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

/// Choose tag column width: prefer [`TAG_COL_WIDTH`], shrink on narrow panes so
/// the message still gets at least 8 columns.
fn tag_col_for_area(area_width: usize, prefix_without_tag: usize) -> usize {
    let reserved = prefix_without_tag + TAG_MSG_GAP + 8;
    let available = area_width.saturating_sub(reserved);
    if available == 0 {
        return 0;
    }
    TAG_COL_WIDTH.min(available).max(TAG_COL_MIN.min(available))
}

/// Fit `tag` into a fixed display-column width: right-pad with spaces, or
/// truncate with `…`. Returns `(display, visible_byte_end)` where
/// `visible_byte_end` is the end of the prefix of `tag` shown before `…`
/// (equals `tag.len()` when not truncated).
fn fit_tag_column(tag: &str, width: usize) -> (String, usize) {
    if width == 0 {
        return (String::new(), 0);
    }
    let tag_w = UnicodeWidthStr::width(tag);
    if tag_w <= width {
        let mut out = tag.to_string();
        out.push_str(&" ".repeat(width - tag_w));
        return (out, tag.len());
    }
    if width == 1 {
        return ("…".to_string(), 0);
    }
    let mut out = String::new();
    let mut used = 0usize;
    let mut byte_end = 0usize;
    for (i, ch) in tag.char_indices() {
        let cw = UnicodeWidthChar::width(ch).unwrap_or(0);
        if used + cw > width - 1 {
            break;
        }
        out.push(ch);
        used += cw;
        byte_end = i + ch.len_utf8();
    }
    out.push('…');
    used += 1;
    if used < width {
        out.push_str(&" ".repeat(width - used));
    }
    (out, byte_end)
}

/// Append tag-column spans (highlights on the visible prefix only) + trailing pad.
fn push_tag_column_spans(
    spans: &mut Vec<Span<'static>>,
    tag: &str,
    tag_col: usize,
    tag_matches: &[ColoredMatch],
    tag_style: Style,
) {
    if tag_col == 0 {
        return;
    }
    let (fitted, visible_end) = fit_tag_column(tag, tag_col);
    let truncated = visible_end < tag.len();
    if tag_matches.is_empty() || visible_end == 0 {
        spans.push(Span::styled(fitted, tag_style));
        return;
    }
    spans.extend(spans_for_range(
        tag,
        (0, visible_end),
        tag_matches,
        tag_style,
    ));
    let mut used = UnicodeWidthStr::width(&tag[..visible_end]);
    if truncated {
        spans.push(Span::styled("…", tag_style));
        used += 1;
    }
    if used < tag_col {
        spans.push(Span::styled(" ".repeat(tag_col - used), tag_style));
    }
}

/// Renders one log entry as one or more physical `Line`s: a header
/// (lineno/timestamp/level/fixed tag column) followed by the message,
/// word-wrapped to `area_width`. The tag field uses a fixed column (pad /
/// truncate with `…`) so messages align; continuation lines indent with
/// spaces matching the header width.
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
    let prefix_without_tag =
        lineno_s.chars().count() + ts.chars().count() + level_badge.chars().count() + LEVEL_TAG_GAP;
    let tag_col = tag_col_for_area(area_width, prefix_without_tag);
    let header_width = prefix_without_tag + tag_col + TAG_MSG_GAP;
    let cont_prefix: String = " ".repeat(header_width);

    let first_width = area_width.saturating_sub(header_width).max(8);
    let cont_width = area_width.saturating_sub(header_width).max(8);

    let tag_style = Style::default()
        .fg(theme::accent())
        .add_modifier(Modifier::BOLD);
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
                spans.push(Span::styled(
                    level_badge.clone(),
                    theme::level_badge_style(row.level),
                ));
                spans.push(Span::styled(" ".repeat(LEVEL_TAG_GAP), Style::default()));
                push_tag_column_spans(&mut spans, &row.tag, tag_col, &tag_matches, tag_style);
                spans.push(Span::styled(" ".repeat(TAG_MSG_GAP), Style::default()));
            } else {
                spans.push(Span::styled(
                    cont_prefix.clone(),
                    Style::default().add_modifier(Modifier::DIM),
                ));
            }
            spans.extend(spans_for_range(
                &row.msg,
                range,
                &msg_matches,
                Style::default(),
            ));
            Line::from(spans)
        })
        .collect()
}

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

    // File: never O(budget) `row_at` — severe via prefetch cache, highlight via
    // async hit index. Stream keeps owned-row sample parse (bounded buffer).
    if app.store.is_file() {
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
            if app.store.as_file().and_then(|f| f.severe_cached(src)) == Some(true) {
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
            if !app.bookmark_alive(bm.row_id) {
                continue;
            }
            if let Some(i) = app.visible_idx_for_row_id(bm.row_id) {
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
    let rail_w = if !app.visible.is_empty() && inner.width > 1 {
        1u16
    } else {
        0
    };
    let content_w = inner.width.saturating_sub(rail_w).max(1);
    let inner_width = content_w as usize;
    let selection = app.selection_range();
    let patterns = app.highlight_groups.paint_patterns(app.active_highlight);

    let lineno_width = app.visible.len().max(1).to_string().len();

    // Compute list_area before building items for the virtual-scroll window size.
    // block.inner() is a pure rect computation — it does not render anything.
    let content_area = Rect {
        x: inner.x,
        y: inner.y,
        width: content_w,
        height: inner.height,
    };
    // M2: bookmark strip embedded at top of Log (collapsed when empty).
    let bm_n = app.bookmarks.display_recent().len() as u16;
    let (bm_area_opt, list_area) = if bm_n > 0 && content_area.height > bm_n {
        let [top, rest] =
            Layout::vertical([Constraint::Length(bm_n), Constraint::Fill(1)]).areas(content_area);
        (Some(top), rest)
    } else {
        (None, content_area)
    };

    // ── Virtual scroll ─────────────────────────────────────────────────────────
    // Build ListItems only for a window around the current scroll position.
    // 3× the viewport height provides a safe margin for multi-line entries.
    let n = app.visible.len();
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
                let mut item = ListItem::new(render_entry_lines(
                    &row,
                    &patterns,
                    inner_width,
                    abs_i + 1,
                    lineno_width,
                ));
                if let Some((lo, hi)) = selection {
                    if abs_i >= lo && abs_i <= hi {
                        item = item.style(theme::log_visual_style());
                    } else if app.is_bookmark_row(row.row_id) {
                        item = item.style(theme::bookmark_row_style());
                    }
                } else if app.is_bookmark_row(row.row_id) {
                    item = item.style(theme::bookmark_row_style());
                } else if active && abs_i == app.cursor {
                    item = item.style(theme::log_selection_style());
                }
                Some(item)
            })
            .collect()
    };

    // Paint border first; list fills the content columns only (no block).
    frame.render_widget(block, area);
    if let Some(area) = bm_area_opt {
        render_bookmark_strip(app, frame, area);
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

/// M2: up to 3 newest bookmarks inside the Log region.
pub fn render_bookmark_strip(app: &App, frame: &mut Frame, area: Rect) {
    use crate::bookmark::fit_label;

    if area.height == 0 {
        return;
    }
    frame.render_widget(Block::default().style(theme::bookmark_strip_style()), area);
    let recent = app.bookmarks.display_recent();
    // " ★ " / " ☆ " prefix is 3 cols; fit the rest to the strip width.
    let text_cols = area.width.saturating_sub(3) as usize;
    let lines: Vec<Line> = recent
        .iter()
        .take(area.height as usize)
        .map(|bm| {
            let alive = app.bookmark_alive(bm.row_id);
            let (mark, style) = if alive {
                ("★", theme::bookmark_label_style())
            } else {
                ("☆", theme::bookmark_stale_style())
            };
            let text = fit_label(&bm.label, text_cols);
            Line::from(Span::styled(format!(" {mark} {text}"), style))
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), area);
}

fn group_dot_span(enabled: bool, selected: bool) -> Span<'static> {
    let dot = if enabled {
        theme::GLYPH_GROUP_ON
    } else {
        theme::GLYPH_GROUP_OFF
    };
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
            let pill = theme::chip_pill_spans(chip.field, &chip.value, !g.enabled);
            spans.extend(pill);
        }
    }
    spans
}

fn highlight_group_spans(
    g: &HighlightGroup,
    color_idx: usize,
    selected: bool,
    active_global: bool,
) -> Vec<Span<'static>> {
    let mut spans = vec![group_dot_span(g.enabled, selected)];
    spans.push(Span::raw(" ".repeat(DOT_PILL_GAP as usize)));
    let pill = theme::highlight_pill_spans(&g.pattern, color_idx, !g.enabled, active_global);
    spans.extend(pill);
    spans
}

fn exclude_entry_spans(
    e: &crate::filter_model::ExcludeEntry,
    selected: bool,
) -> Vec<Span<'static>> {
    let mut spans = vec![group_dot_span(e.enabled, selected)];
    spans.push(Span::raw(" ".repeat(DOT_PILL_GAP as usize)));
    let pill = theme::exclude_pill_spans(e.chip.field, &e.chip.value, !e.enabled);
    spans.extend(pill);
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

fn highlight_strip_lines(app: &App, inner_width: u16) -> Vec<Line<'static>> {
    let active = app.focus == Focus::HighlightStrip;
    let mut color_idx = 0usize;
    let groups: Vec<Vec<Span<'static>>> = app
        .highlight_groups
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
            highlight_group_spans(
                g,
                idx,
                i == app.highlight_cursor && active,
                Some(i) == app.active_highlight,
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
pub fn highlight_strip_height(app: &App, outer_width: u16) -> u16 {
    if app.highlight_groups.groups.is_empty() {
        return 0;
    }
    let inner = outer_width.saturating_sub(2);
    let rows = highlight_strip_lines(app, inner).len().max(1) as u16;
    rows.saturating_add(2)
}

pub fn render_chip_strip(app: &App, frame: &mut Frame, area: Rect) {
    if area.height == 0 {
        return;
    }
    let active = app.focus == Focus::ChipStrip;
    let block = divider_block(theme::numbered_title(1, "Filter", active), active);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(Paragraph::new(filter_strip_lines(app, inner.width)), inner);
}

pub fn render_exclude_chip_strip(app: &App, frame: &mut Frame, area: Rect) {
    if area.height == 0 {
        return;
    }
    let active = app.focus == Focus::ExcludeStrip;
    let block = divider_block(theme::numbered_title(2, "Exclude", active), active);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(Paragraph::new(exclude_strip_lines(app, inner.width)), inner);
}

pub fn render_highlight_chip_strip(app: &App, frame: &mut Frame, area: Rect) {
    if area.height == 0 {
        return;
    }
    let active = app.focus == Focus::HighlightStrip;
    let block = divider_block(theme::numbered_title(3, "Highlight", active), active);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(
        Paragraph::new(highlight_strip_lines(app, inner.width)),
        inner,
    );
}

fn committed_chip_spans(chips: &[crate::input::Chip], exclude_mode: bool) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    for (i, chip) in chips.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw(" ".repeat(PILL_GAP as usize)));
        }
        let pill = if exclude_mode {
            theme::exclude_pill_spans(chip.field, &chip.value, false)
        } else {
            theme::chip_pill_spans(chip.field, &chip.value, false)
        };
        spans.extend(pill);
    }
    spans
}

/// Display-column width of styled spans (sum of content widths).
fn spans_display_width(spans: &[Span<'_>]) -> usize {
    spans.iter().map(|s| s.content.width()).sum()
}

/// Window `text` around `caret` (char index) so the caret stays visible within
/// `max_cols` display columns. Hardware cursor needs no reserved glyph column.
fn window_around_caret(text: &str, caret: usize, max_cols: usize) -> (String, String) {
    let caret = caret.min(text.chars().count());
    let chars: Vec<char> = text.chars().collect();
    let before: String = chars[..caret].iter().collect();
    let after: String = chars[caret..].iter().collect();
    if max_cols == 0 {
        return (String::new(), String::new());
    }
    let bw = before.width();
    let aw = after.width();
    if bw + aw <= max_cols {
        return (before, after);
    }
    if bw <= max_cols {
        let room = max_cols - bw;
        let mut out_after = String::new();
        let mut w = 0;
        for ch in after.chars() {
            let cw = UnicodeWidthChar::width(ch).unwrap_or(0);
            if w + cw > room {
                break;
            }
            out_after.push(ch);
            w += cw;
        }
        return (before, out_after);
    }
    // before too long: keep a suffix ending at the caret.
    let mut w = 0;
    let mut start = caret;
    while start > 0 {
        let ch = chars[start - 1];
        let cw = UnicodeWidthChar::width(ch).unwrap_or(0);
        if w + cw > max_cols {
            break;
        }
        w += cw;
        start -= 1;
    }
    let before_win: String = chars[start..caret].iter().collect();
    let room = max_cols.saturating_sub(w);
    let mut out_after = String::new();
    let mut aw = 0;
    for ch in chars[caret..].iter().copied() {
        let cw = UnicodeWidthChar::width(ch).unwrap_or(0);
        if aw + cw > room {
            break;
        }
        out_after.push(ch);
        aw += cw;
    }
    (before_win, out_after)
}

/// Windowed draft text for hardware-cursor editing.
///
/// Returns `(spans, caret_col)` where `caret_col` is the display-column offset
/// of the caret within the returned draft spans (end of the visible `before`).
pub fn editable_text_spans(
    text: &str,
    caret: usize,
    max_width: Option<u16>,
) -> (Vec<Span<'static>>, u16) {
    let caret = caret.min(text.chars().count());
    let text_budget = max_width.map(|w| w as usize);
    let (before, after) = match text_budget {
        Some(budget) => window_around_caret(text, caret, budget),
        None => {
            let byte = text
                .char_indices()
                .nth(caret)
                .map(|(i, _)| i)
                .unwrap_or(text.len());
            (text[..byte].to_string(), text[byte..].to_string())
        }
    };
    let caret_col = before.width().min(usize::from(u16::MAX)) as u16;
    let mut spans = Vec::new();
    if !before.is_empty() {
        spans.push(Span::styled(before, Style::reset()));
    }
    if !after.is_empty() {
        spans.push(Span::styled(after, Style::reset()));
    }
    (spans, caret_col)
}

/// Build Input draft line spans; returns caret column within `inner` when editing.
fn input_content_spans(
    input: &InputBox,
    show_caret: bool,
    max_width: Option<u16>,
) -> (Vec<Span<'static>>, Option<u16>) {
    let mut spans = committed_chip_spans(&input.chips, input.exclude_mode);
    // Gap + reset after pills so draft never sits inside the pill fill.
    if !input.chips.is_empty() {
        spans.push(Span::styled(" ".repeat(PILL_GAP as usize), Style::reset()));
    }
    if let Some(field) = input.draft_field {
        spans.push(Span::styled(
            format!("{} {}:", theme::field_icon(field), field.keyword()),
            Style::reset().fg(theme::field_color(field)),
        ));
    }
    if show_caret {
        let prefix_w = spans_display_width(&spans);
        let draft_max = max_width.map(|w| (w as usize).saturating_sub(prefix_w) as u16);
        let (draft_spans, caret_col) =
            editable_text_spans(input.draft.as_str(), input.draft.cursor(), draft_max);
        spans.extend(draft_spans);
        let col = (prefix_w as u16).saturating_add(caret_col);
        (spans, Some(col))
    } else {
        spans.push(Span::styled(input.draft.to_string(), Style::reset()));
        (spans, None)
    }
}

/// Centered Input modal (visible while `Focus::Input`).
/// Returns hardware cursor position when in Insert mode.
pub fn render_input_modal(
    input: &InputBox,
    mode: Mode,
    frame: &mut Frame,
    area: Rect,
) -> Option<Position> {
    let title = if input.exclude_mode {
        "Input ! (排除)"
    } else {
        "Input"
    };
    let inner = render_modal_shell(title, frame, area);
    let (spans, caret_col) = input_content_spans(input, mode == Mode::Insert, Some(inner.width));
    frame.render_widget(Paragraph::new(Line::from(spans)), inner);
    caret_col.map(|col| Position {
        x: inner
            .x
            .saturating_add(col.min(inner.width.saturating_sub(1))),
        y: inner.y,
    })
}

/// Legacy single-row Input render kept for unit tests that draw into a fixed area.
pub fn render_input_box(
    input: &InputBox,
    mode: Mode,
    focused: bool,
    frame: &mut Frame,
    area: Rect,
) -> Option<Position> {
    let block = divider_block(theme::numbered_title(5, "Input", focused), focused);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(Clear, inner);
    let (spans, caret_col) = input_content_spans(input, mode == Mode::Insert, Some(inner.width));
    frame.render_widget(Paragraph::new(Line::from(spans)), inner);
    caret_col.map(|col| Position {
        x: inner
            .x
            .saturating_add(col.min(inner.width.saturating_sub(1))),
        y: inner.y,
    })
}

/// Centered Highlight modal: draft row only (history candidates float below).
/// Returns hardware cursor position for the draft caret.
pub fn render_highlight_modal(
    search: &HighlightBox,
    frame: &mut Frame,
    area: Rect,
) -> Option<Position> {
    let inner = render_modal_shell("Highlight", frame, area);
    let (spans, caret_col) = editable_text_spans(
        search.draft.as_str(),
        search.draft.cursor(),
        Some(inner.width),
    );
    frame.render_widget(Paragraph::new(Line::from(spans)), inner);
    Some(Position {
        x: inner
            .x
            .saturating_add(caret_col.min(inner.width.saturating_sub(1))),
        y: inner.y,
    })
}

/// Outer height for H4 Detail modal (border + content), clamped to `frame`.
pub fn detail_modal_height(frame: Rect, content_rows: usize) -> u16 {
    let desired = (content_rows as u16).saturating_add(2).max(3);
    let max = frame.height.saturating_mul(3) / 5;
    let max = max.max(5).min(frame.height.saturating_sub(1));
    desired.min(max).max(3)
}

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
            "无选中行".to_string(),
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
        DetailView::Fields => detail_field_lines(app.current_row().as_deref(), inner_width),
        DetailView::Pretty => detail_pretty_lines(app.current_row().as_deref(), inner_width),
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

/// Outer height for the global time-window panel (`ts`).
pub fn time_panel_height(frame: Rect) -> u16 {
    // border(2) + 4 field rows + section labels(2) + up to 5 candidate rows
    let desired = 13u16;
    let max = frame.height.saturating_mul(3) / 5;
    let max = max.max(8).min(frame.height.saturating_sub(1));
    desired.min(max).max(8)
}

/// Render `ts` time panel. Returns hardware cursor for the focused field.
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
        "日期",
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
        "时间",
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
        "日期",
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
        "时间",
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
    let x = inner
        .x
        .saturating_add(caret_col.min(inner.width.saturating_sub(1)));
    Some(Position { x, y })
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
                    Span::styled(
                        line.text[s..e].to_string(),
                        theme::preview_highlight_style(),
                    ),
                    Span::raw(line.text[e..].to_string()),
                ]))
            } else {
                ListItem::new(Span::raw(line.text.clone()))
            }
        })
        .collect();
    frame.render_widget(List::new(items), inner);
}

/// fzf left-pane committed pills plus search prompt:
/// mode icon (`>` / `＋` / `✎`) + optional `field:` + draft.
/// Returns hardware cursor position for the draft caret.
pub fn render_picker_search_line(
    mode: &crate::picker::PickerMode,
    text: &str,
    caret: usize,
    chips: &[crate::input::Chip],
    exclude_chips: bool,
    draft_field: Option<ChipField>,
    frame: &mut Frame,
    area: Rect,
) -> Option<Position> {
    if area.height == 0 {
        return None;
    }

    let show_chips = !chips.is_empty() && area.height > PICKER_SEARCH_HEIGHT;
    let chip_h = if show_chips { 1 } else { 0 };
    if show_chips {
        let chip_area = Rect {
            x: area.x,
            y: area.y,
            width: area.width,
            height: 1,
        };
        frame.render_widget(
            Paragraph::new(Line::from(committed_chip_spans(chips, exclude_chips))),
            chip_area,
        );
    }

    let input_area = Rect {
        x: area.x,
        y: area.y.saturating_add(chip_h),
        width: area.width,
        height: area.height.saturating_sub(chip_h),
    };
    if input_area.height == 0 {
        return None;
    }

    let block = rounded_block(Line::from(""), true);
    let inner = block.inner(input_area);
    frame.render_widget(block, input_area);
    if inner.width == 0 || inner.height == 0 {
        return None;
    }

    let padding = PICKER_SEARCH_HORIZONTAL_PADDING.min(inner.width / 2);
    let content = Rect {
        x: inner.x.saturating_add(padding),
        y: inner.y,
        width: inner.width.saturating_sub(padding.saturating_mul(2)),
        height: inner.height,
    };
    if content.width == 0 {
        return None;
    }

    let mut prompt_spans = vec![theme::picker_mode_prefix(mode)];
    if let Some(field) = draft_field {
        prompt_spans.push(Span::styled(
            format!("{} {}:", theme::field_icon(field), field.keyword()),
            Style::reset().fg(theme::field_color(field)),
        ));
    }
    let prefix_w = spans_display_width(&prompt_spans);
    let draft_max = (content.width as usize).saturating_sub(prefix_w) as u16;
    let (draft_spans, caret_col) = editable_text_spans(text, caret, Some(draft_max));
    prompt_spans.extend(draft_spans);
    frame.render_widget(Paragraph::new(Line::from(prompt_spans)), content);
    let x_off = (prefix_w as u16).saturating_add(caret_col);
    Some(Position {
        x: content
            .x
            .saturating_add(x_off.min(content.width.saturating_sub(1))),
        y: content.y,
    })
}

/// fzf-style picker shell: left candidates + bottom search, right Preview.
/// Returns hardware cursor position for the search-line caret.
pub fn render_picker(
    title: &str,
    mode: &crate::picker::PickerMode,
    search_text: &str,
    caret: usize,
    match_query: &str,
    chips: &[crate::input::Chip],
    exclude_chips: bool,
    draft_field: Option<ChipField>,
    labels: &[String],
    styles: &[Style],
    checked: &[bool],
    actions: &[ActionKind],
    selected: usize,
    empty_msg: &str,
    preview_lines: &[crate::preview::PreviewLine],
    left_ratio: f32,
    show_preview: bool,
    frame: &mut Frame,
    frame_area: Rect,
) -> Option<Position> {
    let picker_area = picker_frame_rect(frame_area, show_preview);
    frame.render_widget(Clear, picker_area);

    let (left, right) = if show_preview {
        let (l, r) = split_picker_lr_gapped(picker_area, left_ratio);
        (l, Some(r))
    } else {
        (picker_area, None)
    };
    let left_inner = render_modal_shell(title, frame, left);
    let (candidates_area, search_area) = picker_left_stack(left_inner, !chips.is_empty());

    if candidates_area.height > 0 {
        render_candidate_list(
            "list",
            labels,
            styles,
            checked,
            actions,
            selected,
            empty_msg,
            match_query,
            frame,
            candidates_area,
            false,
        );
    }
    let cursor = render_picker_search_line(
        mode,
        search_text,
        caret,
        chips,
        exclude_chips,
        draft_field,
        frame,
        search_area,
    );
    if let Some(right) = right {
        render_preview("Preview", preview_lines, "无预览", frame, right);
    }
    cursor
}

/// Destructive picker action confirmation, overlaid at the picker center.
fn confirm_dialog_question(confirm: &crate::picker::ConfirmKind) -> String {
    match confirm {
        crate::picker::ConfirmKind::DeleteMany { items } => {
            if items.len() == 1 {
                "删除选中？".to_string()
            } else {
                format!("删除选中 {} 项？", items.len())
            }
        }
        crate::picker::ConfirmKind::DeleteBookmark { .. } => "删除书签？".to_string(),
    }
}

pub fn render_confirm_dialog(
    confirm: &crate::picker::ConfirmKind,
    frame: &mut Frame,
    picker_area: Rect,
) {
    let question = confirm_dialog_question(confirm);
    let width = 34.min(picker_area.width).max(1);
    let height = 5.min(picker_area.height).max(1);
    let area = centered_modal_rect(picker_area, width, height);
    let inner = render_modal_shell("确认删除", frame, area);
    let text = vec![
        Line::from(Span::styled(
            question,
            Style::default().add_modifier(Modifier::BOLD),
        ))
        .alignment(Alignment::Center),
        Line::from(Span::styled(
            "y/Enter 确认  n/Esc 取消",
            theme::context_help_style(),
        ))
        .alignment(Alignment::Center),
    ];
    frame.render_widget(Paragraph::new(text).alignment(Alignment::Center), inner);
}

/// Search history-chip candidates — same shell as Input field popup.
pub fn render_highlight_popup(
    search: &HighlightBox,
    groups: &[HighlightGroup],
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
    render_candidate_list(
        "历史",
        &labels,
        &styles,
        &[],
        &[],
        selected,
        "无匹配历史",
        &search.draft,
        frame,
        area,
        true,
    );
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
    render_candidate_list(
        "字段",
        &labels,
        &styles,
        &[],
        &[],
        selected,
        "无匹配字段",
        &input.draft,
        frame,
        area,
        true,
    );
}

pub fn render_status_bar(app: &mut App, frame: &mut Frame, area: Rect) {
    let mut spans = vec![Span::styled(
        format!("{}/{}", app.cursor + 1, app.visible.len()),
        Style::default().add_modifier(Modifier::DIM),
    )];
    if let Some((current, total)) = app.highlight_match_stats() {
        let k = current.map(|n| n.to_string()).unwrap_or_else(|| "-".into());
        spans.push(Span::raw(" "));
        spans.push(theme::status_icon_value(
            theme::GLYPH_SEARCH,
            &format!("{k}/{total}"),
            theme::accent(),
        ));
    }
    if app.following {
        spans.push(Span::raw(" "));
        spans.push(theme::status_icon(theme::GLYPH_FOLLOWING, theme::success()));
    }
    if let Some(lock) = app.lock_badge_label() {
        spans.push(Span::raw(" "));
        spans.push(theme::status_icon_value(
            theme::GLYPH_LOCK,
            &lock,
            theme::lock(),
        ));
    }
    if let Some(time) = app.time_badge_label() {
        spans.push(Span::raw(" "));
        spans.push(theme::status_icon_value(
            theme::GLYPH_TIME,
            &time,
            theme::lock(),
        ));
    }
    if let Some(prog) = app.file_progress_label() {
        spans.push(Span::raw(" "));
        spans.push(theme::status_icon_value(
            theme::GLYPH_PROGRESS,
            &prog,
            theme::warning(),
        ));
    }
    if app.visual_anchor.is_some() {
        spans.push(Span::raw(" "));
        spans.push(theme::status_icon(theme::GLYPH_VISUAL, theme::accent()));
    } else if app.pending_chip {
        spans.push(Span::raw(" "));
        spans.push(theme::status_soft("c…", theme::warning()));
    } else if app.pending_exclude {
        spans.push(Span::raw(" "));
        spans.push(theme::status_soft("C…", theme::warning()));
    } else if app.pending_lock {
        spans.push(Span::raw(" "));
        spans.push(theme::status_soft("f…", theme::warning()));
    } else if app.pending_time {
        spans.push(Span::raw(" "));
        spans.push(theme::status_soft("t…", theme::warning()));
    } else if app.pending_m {
        spans.push(Span::raw(" "));
        spans.push(theme::status_soft("m…", theme::warning()));
    } else if app.pending_yank {
        spans.push(Span::raw(" "));
        spans.push(theme::status_soft("y…", theme::warning()));
    } else if app.pending_d {
        spans.push(Span::raw(" "));
        spans.push(theme::status_soft("d…", theme::warning()));
    } else if app.pending_leader {
        spans.push(Span::raw(" "));
        spans.push(theme::status_soft("SPC…", theme::warning()));
    }
    // Timed flash toast; pending markers above are separate.
    if let Some(msg) = &app.status_msg {
        spans.push(Span::raw(" "));
        let fg = if msg.starts_with("YANK FAILED") {
            theme::warning()
        } else {
            theme::accent()
        };
        spans.push(theme::status_soft(msg, fg));
    }
    // Trailing context help: badges keep priority; hint truncates or hides.
    let left_width: usize = spans.iter().map(span_width).sum();
    let avail = (area.width as usize)
        .saturating_sub(left_width)
        .saturating_sub(1); // leading space before the hint
    if let Some(hint) = crate::help::context_hint_spans(app, avail) {
        spans.push(Span::raw(" "));
        spans.extend(hint);
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// Read-only Help panel (`?`): Active context + full catalog.
pub fn render_help_panel(app: &App, frame: &mut Frame, area: Rect) {
    let title = format!("{} Help", theme::GLYPH_HELP);
    let inner = render_modal_shell(&title, frame, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }
    let lines = crate::help::help_body_lines(app);
    let scroll = app.help_scroll.min(lines.len().saturating_sub(1));
    let visible: Vec<Line<'static>> = lines.into_iter().skip(scroll).collect();
    frame.render_widget(
        Paragraph::new(visible).wrap(ratatui::widgets::Wrap { trim: false }),
        inner,
    );
}

/// Height for the Help modal given frame size and content.
pub fn help_modal_height(frame: Rect, content_rows: usize) -> u16 {
    let max = frame.height.saturating_sub(4).max(8);
    let want = (content_rows as u16).saturating_add(2); // border
    want.min(max).max(8)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::highlight_model::HighlightGroup;
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
        tx.send(
            crate::model::EntryRow::from_line("04-02 10:00:00.000  1  1 I MyTag   : hello world")
                .unwrap(),
        )
        .unwrap();
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
        tx.send(
            crate::model::EntryRow::from_line("04-02 10:00:00.000  1  1 I RowOne  : first")
                .unwrap(),
        )
        .unwrap();
        tx.send(
            crate::model::EntryRow::from_line("04-02 10:00:01.000  1  1 I RowTwo  : second")
                .unwrap(),
        )
        .unwrap();
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
        assert_eq!(
            selected_style.bg,
            Some(Color::DarkGray),
            "focused selection must use the soft gray background"
        );
        assert_ne!(
            unselected_style.bg,
            Some(Color::DarkGray),
            "unselected rows must not get the selection background"
        );
        assert!(
            !selected_style.add_modifier.contains(Modifier::REVERSED),
            "must not use the old reverse-video style"
        );
    }

    #[test]
    fn test_render_log_list_no_highlight_when_log_list_unfocused() {
        let mut app = App::new(100);
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(
            crate::model::EntryRow::from_line("04-02 10:00:00.000  1  1 I RowOne  : first")
                .unwrap(),
        )
        .unwrap();
        tx.send(
            crate::model::EntryRow::from_line("04-02 10:00:01.000  1  1 I RowTwo  : second")
                .unwrap(),
        )
        .unwrap();
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
        tx.send(
            crate::model::EntryRow::from_line("04-02 10:00:00.000  1  1 I Tag     : an error here")
                .unwrap(),
        )
        .unwrap();
        drop(tx);
        app.drain(&rx);
        app.highlight_groups
            .groups
            .push(HighlightGroup::from_pattern("error").unwrap());
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
    fn test_render_log_list_bookmark_row_bg_priority() {
        // AC1: bookmarked rows get a faint-yellow bg; an active visual selection
        // overrides it; the cursor-selection gray only applies when neither
        // visual nor bookmark bg is present. Priority: visual > bookmark-bg > cursor.
        // The whole buffer is scanned (bookmark strip shifts list content, so
        // fixed row coords are unreliable).
        let mut app = App::new(100);
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(
            crate::model::EntryRow::from_line("04-02 10:00:00.000  1  1 I TagA   : first").unwrap(),
        )
        .unwrap();
        tx.send(
            crate::model::EntryRow::from_line("04-02 10:00:01.000  1  1 I TagB   : second")
                .unwrap(),
        )
        .unwrap();
        tx.send(
            crate::model::EntryRow::from_line("04-02 10:00:02.000  1  1 I TagC   : third").unwrap(),
        )
        .unwrap();
        drop(tx);
        app.drain(&rx);
        app.following = false;

        // Bookmark rows 0 and 1; row 2 stays plain.
        app.cursor = 0;
        app.bookmark_add_current();
        app.cursor = 1;
        let bm_bg = theme::bookmark_row_style().bg;
        let vis_bg = theme::log_visual_style().bg;
        let sel_bg = theme::log_selection_style().bg;

        let scan_bg_in_rows =
            |terminal: &Terminal<TestBackend>, target: Option<Color>, y0: u16, y1: u16| -> bool {
                let buf = terminal.backend().buffer();
                for y in y0..y1 {
                    for x in 0..buf.area.width {
                        if buf[(x, y)].style().bg == target {
                            return true;
                        }
                    }
                }
                false
            };

        // Case 1: cursor on row 2 (focused, no visual selection). Rows 0 and 1
        // are bookmarked but not the cursor row → they get the bookmark bg;
        // the cursor row gets the selection gray.
        app.cursor = 2;
        app.focus = Focus::LogList;
        let backend = TestBackend::new(80, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render_log_list(&mut app, frame, frame.area()))
            .unwrap();
        assert!(
            scan_bg_in_rows(&terminal, bm_bg, 0, terminal.backend().buffer().area.height),
            "bookmarked non-cursor row must get the bookmark bg"
        );
        assert!(
            scan_bg_in_rows(
                &terminal,
                sel_bg,
                0,
                terminal.backend().buffer().area.height
            ),
            "focused cursor row must get the selection bg"
        );

        // Case 2: enter visual-line on row 0, extend cursor to row 1. Both
        // bookmarked rows are inside the visual range → visual overrides bookmark.
        // The bookmark strip occupies the top rows (1 border + 2 strip rows),
        // so list content starts at y=3; rows 0,1 land at y=3,4 and must carry
        // the visual bg, NOT the bookmark bg.
        app.enter_visual_line(); // anchor at row 0
        app.cursor = 1; // range [0,1]
        terminal
            .draw(|frame| render_log_list(&mut app, frame, frame.area()))
            .unwrap();
        assert!(
            scan_bg_in_rows(
                &terminal,
                vis_bg,
                3,
                terminal.backend().buffer().area.height
            ),
            "visual selection must override bookmark bg on list rows"
        );
        assert!(
            !scan_bg_in_rows(&terminal, bm_bg, 3, terminal.backend().buffer().area.height),
            "bookmark bg must yield to visual selection on list rows"
        );
    }

    #[test]
    fn test_render_log_list_persists_scroll_offset_when_cursor_moves_within_viewport() {
        let mut app = App::new(100);
        let (tx, rx) = std::sync::mpsc::channel();
        for i in 0..30 {
            tx.send(
                crate::model::EntryRow::from_line(&format!(
                    "04-02 10:00:00.000  1  1 I Tag     : line{i}"
                ))
                .unwrap(),
            )
            .unwrap();
        }
        drop(tx);
        app.drain(&rx);
        app.following = false;

        // Backend height 7 minus 2 border rows leaves a 5-row inner viewport.
        let backend = TestBackend::new(80, 7);
        let mut terminal = Terminal::new(backend).unwrap();

        app.cursor = 20;
        terminal
            .draw(|frame| render_log_list(&mut app, frame, frame.area()))
            .unwrap();
        let offset_at_edge = app.list_offset;
        assert!(
            offset_at_edge > 0,
            "cursor near the bottom of a 30-row list in a 5-row viewport must have scrolled"
        );

        app.cursor -= 2; // moves, but stays inside the already-scrolled viewport
        terminal
            .draw(|frame| render_log_list(&mut app, frame, frame.area()))
            .unwrap();

        assert_eq!(
            app.list_offset, offset_at_edge,
            "moving within the visible window must not re-scroll the viewport"
        );
    }

    #[test]
    fn test_wrap_ranges_breaks_on_whitespace() {
        let ranges = wrap_ranges("hello world foo", 11);
        let chunks: Vec<&str> = ranges
            .iter()
            .map(|&(s, e)| &"hello world foo"[s..e])
            .collect();
        assert_eq!(chunks, vec!["hello world", "foo"]);
    }

    #[test]
    fn test_wrap_ranges_hard_cuts_overlong_word() {
        let text = "supercalifragilistic";
        let ranges = wrap_ranges(text, 6);
        assert!(
            ranges.len() > 1,
            "an overlong word must be split into multiple pieces"
        );
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
        assert!(
            lines.len() > 1,
            "a long message should wrap into multiple lines, got {}",
            lines.len()
        );
    }

    #[test]
    fn test_render_entry_lines_highlights_only_matched_keyword() {
        let row =
            EntryRow::from_line("04-02 10:00:00.000  1  1 I Tag     : an error occurred here")
                .unwrap();
        let re = Regex::new("(?i)error").unwrap();
        let patterns = [(&re, 0usize, false)];
        let lines = render_entry_lines(&row, &patterns, 200, 1, 1);
        assert_eq!(lines.len(), 1);
        let matched: Vec<&Span> = lines[0]
            .spans
            .iter()
            .filter(|s| s.content.as_ref() == "error")
            .collect();
        assert_eq!(
            matched.len(),
            1,
            "exactly the matched keyword should be its own span"
        );
        let other_span_styles: Vec<Style> = lines[0]
            .spans
            .iter()
            .filter(|s| s.content.as_ref() != "error")
            .map(|s| s.style)
            .collect();
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
            Style::default()
                .fg(theme::accent())
                .add_modifier(Modifier::BOLD)
        );
    }

    #[test]
    fn test_render_entry_lines_multicolor_patterns() {
        let row =
            EntryRow::from_line("04-02 10:00:00.000  1  1 I Tag     : foo and bar here").unwrap();
        let re0 = Regex::new("(?i)foo").unwrap();
        let re1 = Regex::new("(?i)bar").unwrap();
        let patterns = [(&re0, 0usize, true), (&re1, 1usize, false)];
        let lines = render_entry_lines(&row, &patterns, 200, 1, 1);
        let foo = lines[0]
            .spans
            .iter()
            .find(|s| s.content.as_ref() == "foo")
            .unwrap();
        let bar = lines[0]
            .spans
            .iter()
            .find(|s| s.content.as_ref() == "bar")
            .unwrap();
        assert_eq!(foo.style, theme::highlight_style_active(0));
        assert_eq!(bar.style, theme::highlight_style(1));
        assert!(foo.style.add_modifier.contains(Modifier::UNDERLINED));
        assert!(!bar.style.add_modifier.contains(Modifier::UNDERLINED));
        assert_ne!(foo.style.bg, bar.style.bg);
    }

    #[test]
    fn test_render_entry_lines_pads_short_tag_to_fixed_column() {
        let row = EntryRow::from_line("04-02 10:00:00.000  1  1 I Ab   : msg").unwrap();
        let lines = render_entry_lines(&row, &[], 200, 1, 1);
        let tag_span = lines[0]
            .spans
            .iter()
            .find(|s| s.content.as_ref().starts_with("Ab"))
            .expect("tag span");
        assert_eq!(
            tag_span.content.as_ref(),
            format!("{:width$}", "Ab", width = TAG_COL_WIDTH),
            "short tag must pad to fixed tag column"
        );
        // level badge then a plain gap (no badge fill) before the tag column
        let contents: Vec<&str> = lines[0].spans.iter().map(|s| s.content.as_ref()).collect();
        let level_idx = contents
            .iter()
            .position(|c| *c == " I ")
            .expect("level badge");
        assert_eq!(contents[level_idx + 1], " ", "gap after level badge");
        assert!(
            contents[level_idx + 2].starts_with("Ab"),
            "tag column follows the gap"
        );
    }

    #[test]
    fn test_render_entry_lines_truncates_long_tag_in_fixed_column() {
        let long = "A".repeat(TAG_COL_WIDTH + 8);
        let row =
            EntryRow::from_line(&format!("04-02 10:00:00.000  1  1 I {long}   : msg")).unwrap();
        let lines = render_entry_lines(&row, &[], 200, 1, 1);
        let tag_span = lines[0]
            .spans
            .iter()
            .find(|s| s.content.as_ref().contains('…'))
            .expect("truncated tag span");
        assert_eq!(
            UnicodeWidthStr::width(tag_span.content.as_ref()),
            TAG_COL_WIDTH
        );
        assert!(
            tag_span.content.as_ref().ends_with('…') || tag_span.content.as_ref().contains('…'),
            "long tag must use ellipsis"
        );
        let text: String = lines[0].spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("msg"), "message must still be visible");
        assert!(
            !text.contains(&long),
            "full long tag must not spill into the line"
        );
    }

    #[test]
    fn test_render_entry_lines_continuation_indent_matches_header_width() {
        let row = EntryRow::from_line(
            "04-02 10:00:00.000  1  1 I Short : this message is long enough that it must wrap across more than one physical line when the column width is narrow",
        )
        .unwrap();
        let area_width = 40;
        let lines = render_entry_lines(&row, &[], area_width, 1, 1);
        assert!(lines.len() > 1);
        let lineno_s = "1 ";
        let ts = "04-02 10:00:00.000 ";
        let level = " I ";
        let prefix_without_tag =
            lineno_s.chars().count() + ts.chars().count() + level.chars().count() + LEVEL_TAG_GAP;
        let tag_col = tag_col_for_area(area_width, prefix_without_tag);
        let header_width = prefix_without_tag + tag_col + TAG_MSG_GAP;
        let cont = lines[1].spans[0].content.as_ref();
        assert!(
            cont.chars().all(|c| c == ' '),
            "continuation prefix should be spaces"
        );
        assert_eq!(cont.chars().count(), header_width);
    }

    #[test]
    fn test_fit_tag_column_pads_and_truncates() {
        let (short, end) = fit_tag_column("Ab", 6);
        assert_eq!(short, "Ab    ");
        assert_eq!(end, 2);
        let (long, end) = fit_tag_column("abcdefghij", 6);
        assert_eq!(long, "abcde…");
        assert_eq!(end, 5);
    }

    #[test]
    fn test_render_entry_lines_shows_lineno_without_pid_tid() {
        let row = EntryRow::from_line("04-02 10:00:00.000  1234  5678 I MyTag   : hello").unwrap();
        let lines = render_entry_lines(&row, &[], 200, 12, 3);
        let text: String = lines[0].spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(
            text.contains(" 12 "),
            "lineno should be right-aligned to width 3"
        );
        assert!(text.contains("MyTag"));
        assert!(text.contains("hello"));
        assert!(
            !text.contains("1234"),
            "pid must not appear in default display"
        );
        assert!(
            !text.contains("5678"),
            "tid must not appear in default display"
        );
        assert!(text.contains(" I "), "level badge must remain");
    }

    #[test]
    fn test_chip_pill_and_highlight_pill_styles() {
        let (text, body) = theme::chip_pill_style(crate::input::ChipField::Tag, "MyTag", false);
        assert!(text.contains("MyTag"));
        assert_eq!(body.bg, Some(theme::accent()));
        let (_, disabled) = theme::chip_pill_style(crate::input::ChipField::Msg, "x", true);
        assert_eq!(disabled, theme::disabled_chip_style());
        let (_, search) = theme::highlight_pill_style("error", 0, false, false);
        assert_eq!(search, theme::highlight_style(0));
        let (_, active) = theme::highlight_pill_style("error", 0, false, true);
        assert_eq!(active, theme::highlight_style_active(0));
        assert!(active.add_modifier.contains(Modifier::UNDERLINED));
    }

    #[test]
    fn test_render_input_box_hw_cursor_after_committed_pill() {
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
        let mut cursor = None;
        terminal
            .draw(|frame| {
                cursor = render_input_box(&input, Mode::Insert, true, frame, frame.area());
                if let Some(pos) = cursor {
                    frame.set_cursor_position(pos);
                }
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        let content = cell_text(buf);
        assert!(content.contains("MyTag"));
        assert!(content.contains('x'));
        let pos = cursor.expect("Insert mode must report a hardware cursor");
        // Cursor sits after draft char 'x' on the content row.
        assert!(pos.x > 0, "cursor x={pos:?}");
        assert_eq!(pos.y, 1);
    }

    #[test]
    fn test_chip_strip_selection_keeps_stable_layout() {
        use crate::filter_model::Group;
        use crate::input::{Chip, ChipField};

        let mut app = App::new(100);
        app.groups.groups.push(Group {
            label: "a".into(),
            chips: vec![Chip {
                field: ChipField::Tag,
                value: "A".into(),
            }],
            expr: None,
            enabled: true,
        });
        app.groups.groups.push(Group {
            label: "b".into(),
            chips: vec![Chip {
                field: ChipField::Msg,
                value: "B".into(),
            }],
            expr: None,
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

        // Selection only restyles the group dot — glyph layout stays put.
        assert!(before.contains('A') && after.contains('A'));
        assert!(before.contains('B') && after.contains('B'));
        assert_eq!(
            before.chars().filter(|c| *c == 'A' || *c == 'B').count(),
            after.chars().filter(|c| *c == 'A' || *c == 'B').count()
        );
        // divider_block draws top + bottom horizontal rules (─), no rounded corners.
        let rules = before.chars().filter(|c| *c == '─').count();
        assert!(
            rules >= 2,
            "strip should have top+bottom ─ rules, got {rules}"
        );
        let rounded = before.chars().filter(|c| "╭╮╰╯".contains(*c)).count();
        assert_eq!(
            rounded, 0,
            "Filter strip must stay divider-only (no rounded modal corners)"
        );
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
                enabled: true,
            });
        }
        let h = filter_strip_height(&app, 20);
        assert!(
            h > 3,
            "wrapped strip should exceed one content row + chrome, got {h}"
        );
        assert_eq!(
            filter_strip_height(&app, 20),
            h,
            "height is instantaneous (stable)"
        );
        app.groups.groups.clear();
        assert_eq!(filter_strip_height(&app, 20), 0);
    }

    #[test]
    fn test_render_status_bar_shows_highlight_match_stats() {
        let mut app = App::new(100);
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(crate::model::EntryRow::from_line("04-02 10:00:00.000  1  1 I T   : aaa").unwrap())
            .unwrap();
        tx.send(
            crate::model::EntryRow::from_line("04-02 10:00:01.000  1  1 I T   : hit one").unwrap(),
        )
        .unwrap();
        tx.send(
            crate::model::EntryRow::from_line("04-02 10:00:02.000  1  1 I T   : hit two").unwrap(),
        )
        .unwrap();
        drop(tx);
        app.drain(&rx);
        app.following = false;
        app.cursor = 0;
        app.push_or_find_highlight_group(HighlightGroup::from_pattern("hit").unwrap());

        let backend = TestBackend::new(40, 1);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render_status_bar(&mut app, frame, frame.area()))
            .unwrap();
        let content = cell_text(terminal.backend().buffer());
        assert!(
            content.contains("-/2"),
            "cursor not on hit: got {content:?}"
        );
        assert!(
            !content.contains("[-/2]"),
            "match stats must not use brackets: got {content:?}"
        );

        app.cursor = 1;
        terminal
            .draw(|frame| render_status_bar(&mut app, frame, frame.area()))
            .unwrap();
        let content = cell_text(terminal.backend().buffer());
        assert!(
            content.contains("1/2"),
            "first hit ordinal: got {content:?}"
        );
    }

    #[test]
    fn test_render_status_bar_shows_context_help_when_wide() {
        let mut app = App::new(100);
        app.following = false;
        app.focus = Focus::LogList;

        let backend = TestBackend::new(80, 1);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render_status_bar(&mut app, frame, frame.area()))
            .unwrap();
        let content = cell_text(terminal.backend().buffer());
        assert!(
            content.contains("j/k") && content.contains("Esc") && content.contains("help"),
            "wide bar should show LogList help: got {content:?}"
        );
    }

    #[test]
    fn test_render_status_bar_hides_context_help_when_narrow() {
        let mut app = App::new(100);
        app.following = true; // follow icon consumes space
        app.focus = Focus::LogList;

        // Wide enough for "1/0" + follow icon, too tight for help (avail < 8).
        let backend = TestBackend::new(12, 1);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render_status_bar(&mut app, frame, frame.area()))
            .unwrap();
        let content = cell_text(terminal.backend().buffer());
        assert!(
            content.contains(theme::GLYPH_FOLLOWING),
            "follow icon must win over help: got {content:?}"
        );
        assert!(
            !content.contains("help") && !content.contains("j/k"),
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
                EntryRow::from_line(&format!("04-02 10:00:00.000  1  1 {level} Tag     : {msg}"))
                    .unwrap(),
            )
            .unwrap();
        }
        drop(tx);
        app.drain(&rx);
        app.highlight_groups
            .groups
            .push(HighlightGroup::from_pattern("findme").unwrap());
        app.list_offset = 0;

        let marks = build_minimap_marks(&app, 4);
        assert_eq!(marks.len(), 4);
        assert!(marks.iter().any(|m| *m == MinimapMark::Severe));
        assert!(marks.iter().any(|m| *m == MinimapMark::Highlight));
        assert!(marks.iter().any(|m| *m == MinimapMark::Viewport));
        // Index 1 (E) maps near row 1 of 4.
        assert_eq!(marks[minimap_row_for_index(1, 4, 4)], MinimapMark::Severe);
    }

    #[test]
    fn test_build_minimap_marks_file_uses_cache_and_hit_index() {
        use crate::store::FileStore;
        use std::io::Write;

        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(
            b"04-02 10:00:00.000  1  1 I Tag     : ok\n\
              04-02 10:00:01.000  1  1 E Tag     : boom\n\
              04-02 10:00:02.000  1  1 I Tag     : findme here\n\
              04-02 10:00:03.000  1  1 I Tag     : ok\n",
        )
        .unwrap();
        f.flush().unwrap();
        let mut app = App::new(100);
        app.set_file_store(FileStore::open_sync(f.path()).unwrap());
        // Wait for severe prefetch so cache is warm (no UI row_at needed).
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        loop {
            let done = app
                .store
                .as_file()
                .map(|fs| fs.progress().severe_done)
                .unwrap_or(true);
            if done {
                break;
            }
            if std::time::Instant::now() > deadline {
                panic!("severe prefetch timed out");
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        app.highlight_scan.hits = vec![2];
        app.highlight_scan.done = true;
        app.list_offset = 0;

        let marks = build_minimap_marks(&app, 4);
        assert!(marks.iter().any(|m| *m == MinimapMark::Severe));
        assert!(marks.iter().any(|m| *m == MinimapMark::Highlight));
        assert_eq!(marks[minimap_row_for_index(1, 4, 4)], MinimapMark::Severe);
        assert_eq!(
            marks[minimap_row_for_index(2, 4, 4)],
            MinimapMark::Highlight
        );
    }

    #[test]
    fn test_build_minimap_marks_bookmark_over_highlight() {
        // F5: a bookmarked alive row produces a Bookmark mark; on overlap with
        // a Highlight mark, Bookmark wins (Severe > Bookmark > Highlight).
        let mut app = App::new(100);
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(EntryRow::from_line("04-02 10:00:00.000  1  1 I Tag     : findme").unwrap())
            .unwrap();
        tx.send(EntryRow::from_line("04-02 10:00:01.000  1  1 I Tag     : other").unwrap())
            .unwrap();
        drop(tx);
        app.drain(&rx);
        app.highlight_groups
            .groups
            .push(HighlightGroup::from_pattern("findme").unwrap());
        app.cursor = 0;
        app.bookmark_add_current();
        app.list_offset = 0;

        let marks = build_minimap_marks(&app, 4);
        assert!(
            marks.iter().any(|m| *m == MinimapMark::Bookmark),
            "bookmark row yields a Bookmark mark: {marks:?}"
        );
        // Index 0 is both highlighted and bookmarked; Bookmark must win there.
        assert_eq!(marks[minimap_row_for_index(0, 2, 4)], MinimapMark::Bookmark);
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
        assert!(
            found_mark,
            "minimap rail should paint │/• inside the log border"
        );
    }

    #[test]
    fn split_picker_lr_respects_ratio() {
        let area = Rect::new(0, 0, 100, 40);
        let (l, r) = split_picker_lr(area, 0.4);
        assert_eq!(l.width, 40);
        assert_eq!(r.width, 60);
        assert_eq!(l.height, r.height);
    }

    #[test]
    fn picker_left_stack_search_at_bottom() {
        let left = Rect::new(0, 0, 40, 20);
        let (cand, search) = picker_left_stack(left, false);
        assert_eq!(search.height, 3);
        assert_eq!(search.y + search.height, left.y + left.height);
        assert_eq!(cand.y, left.y);
        assert_eq!(cand.height, left.height - 3);

        let (cand, search) = picker_left_stack(left, true);
        assert_eq!(search.height, 4);
        assert_eq!(search.y + search.height, left.y + left.height);
        assert_eq!(cand.height, left.height - 4);
    }

    #[test]
    fn editable_text_spans_follow_caret_when_truncated() {
        let (spans, caret_col) = editable_text_spans("abcdefghij", 10, Some(5));
        let joined: String = spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(
            joined.contains('j'),
            "end char must remain visible: {joined:?}"
        );
        assert!(
            !joined.contains('a'),
            "start char should scroll off: {joined:?}"
        );
        assert_eq!(caret_col as usize, UnicodeWidthStr::width(joined.as_str()));
    }

    #[test]
    fn editable_text_spans_mid_caret_is_plain_text() {
        let (spans, caret_col) = editable_text_spans("abcdef", 2, None);
        let joined: String = spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(joined, "abcdef");
        assert_eq!(caret_col, 2);
        assert!(!joined.contains('▏'));
    }

    #[test]
    fn editable_text_spans_start_caret_col_is_zero() {
        let (spans, caret_col) = editable_text_spans("ab", 0, None);
        let joined: String = spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(joined, "ab");
        assert_eq!(caret_col, 0);
    }

    #[test]
    fn picker_search_line_has_rounded_border_padding_and_icon_gap() {
        let backend = TestBackend::new(20, 3);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut cursor = None;
        terminal
            .draw(|frame| {
                cursor = render_picker_search_line(
                    &crate::picker::PickerMode::Manage,
                    "abc",
                    3,
                    &[],
                    false,
                    None,
                    frame,
                    frame.area(),
                );
            })
            .unwrap();

        let buf = terminal.backend().buffer();
        assert_eq!(buf[(0, 0)].symbol(), "╭");
        assert_eq!(buf[(19, 0)].symbol(), "╮");
        assert_eq!(buf[(0, 2)].symbol(), "╰");
        assert_eq!(buf[(19, 2)].symbol(), "╯");
        assert_eq!(buf[(1, 1)].symbol(), " ");
        assert_eq!(buf[(2, 1)].symbol(), theme::GLYPH_MODE_MANAGE);
        assert_eq!(buf[(3, 1)].symbol(), " ");
        assert_eq!(buf[(4, 1)].symbol(), "a");
        assert_eq!(cursor, Some(Position { x: 7, y: 1 }));
    }

    #[test]
    fn picker_search_line_keeps_committed_chips_above_border() {
        use crate::input::{Chip, ChipField};

        let chips = vec![Chip {
            field: ChipField::Tag,
            value: "MyTag".into(),
        }];
        let backend = TestBackend::new(40, 4);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                let _ = render_picker_search_line(
                    &crate::picker::PickerMode::Edit { index: 0 },
                    "",
                    0,
                    &chips,
                    false,
                    None,
                    frame,
                    frame.area(),
                );
            })
            .unwrap();

        let buf = terminal.backend().buffer();
        let content = cell_text(buf);
        assert!(content.lines().next().unwrap_or_default().contains("MyTag"));
        assert_eq!(buf[(0, 1)].symbol(), "╭");
        assert_eq!(buf[(39, 3)].symbol(), "╯");
    }

    #[test]
    fn picker_search_area_shows_committed_chip() {
        use crate::input::{Chip, ChipField};

        let chips = vec![Chip {
            field: ChipField::Tag,
            value: "MyTag".into(),
        }];
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                let _ = render_picker(
                    "Filter · Edit",
                    &crate::picker::PickerMode::Edit { index: 0 },
                    "",
                    0,
                    "",
                    &chips,
                    false,
                    None,
                    &[],
                    &[],
                    &[],
                    &[],
                    0,
                    "无项目",
                    &[],
                    0.4,
                    true,
                    frame,
                    frame.area(),
                );
            })
            .unwrap();

        let content = cell_text(terminal.backend().buffer());
        assert!(content.contains("MyTag"));
    }

    #[test]
    fn picker_search_area_shows_confirmed_draft_field() {
        use crate::input::ChipField;

        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                let _ = render_picker(
                    "Filter · New",
                    &crate::picker::PickerMode::New,
                    "",
                    0,
                    "",
                    &[],
                    false,
                    Some(ChipField::Tag),
                    &[],
                    &[],
                    &[],
                    &[],
                    0,
                    "无项目",
                    &[],
                    0.4,
                    true,
                    frame,
                    frame.area(),
                );
            })
            .unwrap();

        let content = cell_text(terminal.backend().buffer());
        assert!(
            content.contains("tag:"),
            "confirmed field must appear as tag: prefix, got: {content:?}"
        );
    }

    #[test]
    fn confirm_dialog_renders_delete_one_copy_over_picker() {
        use crate::picker::{ConfirmKind, UnifiedId, UnifiedKind};

        let confirm = ConfirmKind::DeleteMany {
            items: vec![UnifiedId {
                kind: UnifiedKind::Highlight,
                source_index: 0,
            }],
        };
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                render_picker(
                    "Manage",
                    &crate::picker::PickerMode::Manage,
                    "",
                    0,
                    "",
                    &[],
                    false,
                    None,
                    &["error".into()],
                    &[theme::muted()],
                    &[],
                    &[],
                    0,
                    "无项目",
                    &[],
                    0.4,
                    true,
                    frame,
                    frame.area(),
                );
                let picker_area = picker_frame_rect(frame.area(), true);
                render_confirm_dialog(&confirm, frame, picker_area);
            })
            .unwrap();

        let content = cell_text(terminal.backend().buffer());
        assert_eq!(confirm_dialog_question(&confirm), "删除选中？");
        assert!(content.contains("y/Enter"));
        assert!(content.contains("n/Esc"));
    }

    #[test]
    fn confirm_dialog_renders_delete_many_count() {
        use crate::picker::{ConfirmKind, UnifiedId, UnifiedKind};

        let confirm = ConfirmKind::DeleteMany {
            items: (0..12)
                .map(|i| UnifiedId {
                    kind: UnifiedKind::Filter,
                    source_index: i,
                })
                .collect(),
        };
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                let picker_area = picker_frame_rect(frame.area(), true);
                render_confirm_dialog(&confirm, frame, picker_area);
            })
            .unwrap();

        let content = cell_text(terminal.backend().buffer());
        assert_eq!(confirm_dialog_question(&confirm), "删除选中 12 项？");
        assert!(content.contains("12"));
    }

    #[test]
    fn picker_frame_rect_compact_when_no_preview() {
        let frame = Rect::new(0, 0, 100, 40);
        let full = picker_frame_rect(frame, true);
        let compact = picker_frame_rect(frame, false);
        assert_eq!(compact.width, full.width / 2);
        assert_eq!(compact.height, full.height);
        assert_eq!(
            compact.x,
            frame.x + (frame.width.saturating_sub(compact.width)) / 2
        );
    }

    #[test]
    fn split_picker_lr_gapped_leaves_one_col() {
        let area = Rect::new(0, 0, 100, 40);
        let (l, r) = split_picker_lr_gapped(area, 0.4);
        assert_eq!(l.x + l.width + 1, r.x);
        assert_eq!(l.width + r.width + 1, area.width);
    }

    #[test]
    fn modal_shell_draws_rounded_corners() {
        let backend = TestBackend::new(40, 8);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                let area = Rect::new(2, 1, 36, 5);
                let _ = render_modal_shell("Input", frame, area);
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        // Rounded BorderType corners (ratatui): ╭ ╮ ╰ ╯
        let mut corners = 0u32;
        for y in 1..6u16 {
            for x in 2..38u16 {
                match buf[(x, y)].symbol() {
                    "╭" | "╮" | "╰" | "╯" => corners += 1,
                    _ => {}
                }
            }
        }
        assert_eq!(corners, 4, "modal shell should paint four rounded corners");
    }

    #[test]
    fn confirm_dialog_centers_on_compact_picker() {
        use crate::picker::{ConfirmKind, UnifiedId, UnifiedKind};

        let confirm = ConfirmKind::DeleteMany {
            items: vec![UnifiedId {
                kind: UnifiedKind::Filter,
                source_index: 0,
            }],
        };
        let frame = Rect::new(0, 0, 100, 40);
        let compact = picker_frame_rect(frame, false);
        let full = picker_frame_rect(frame, true);
        let width = 34.min(compact.width).max(1);
        let height = 5.min(compact.height).max(1);
        let area = centered_modal_rect(compact, width, height);
        assert!(
            area.x >= compact.x
                && area.x + area.width <= compact.x + compact.width
                && area.y >= compact.y
                && area.y + area.height <= compact.y + compact.height,
            "confirm area {area:?} must lie inside compact picker {compact:?}"
        );
        assert!(
            compact.width * 2 <= full.width + 1,
            "compact width should be ~half of full ({compact:?} vs {full:?})"
        );

        let backend = TestBackend::new(100, 40);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                render_confirm_dialog(&confirm, f, compact);
            })
            .unwrap();
        let content = cell_text(terminal.backend().buffer());
        assert!(content.contains("y/Enter"));
        assert!(content.contains("n/Esc"));
    }
}
