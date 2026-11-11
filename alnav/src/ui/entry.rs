//! Log row painting: wrap ranges, colored match spans, tag column fitting and entry lines.

use crate::fuzzy::PaintField;
use crate::model::EntryRow;
use crate::theme;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// LogList tag column width (display columns); short tags pad, long tags truncate.
pub(super) const TAG_COL_WIDTH: usize = 20;

/// Preview pane tag column cap (narrower than LogList so msg keeps budget).
const PREVIEW_TAG_COL_MAX: usize = 12;

/// Floor for the tag column when the pane is narrow (still may shrink further).
const TAG_COL_MIN: usize = 4;

/// Gap between level badge and tag column (outside badge fill).
pub(super) const LEVEL_TAG_GAP: usize = 1;

/// Gap between the fixed tag column and the message.
pub(super) const TAG_MSG_GAP: usize = 1;

/// Greedy word-wrap: returns byte ranges into `text`, one per physical
/// line, breaking on whitespace where possible. A single word longer than
/// `width` is hard-cut into `width`-sized pieces (never infinite-loops).
/// Leading/trailing whitespace around words is dropped; interior spacing
/// between words on the same line is preserved verbatim.
pub(super) fn wrap_ranges(text: &str, width: usize) -> Vec<(usize, usize)> {
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

/// Paint pattern: fuzzy pattern, color index, whether this is the globally active search.
pub(super) type PaintPattern<'a> = (&'a str, usize, bool);

/// Merge a new interval into non-overlapping `result` (later pattern wins on overlap).
fn merge_colored_match(
    result: &mut Vec<ColoredMatch>,
    ns: usize,
    ne: usize,
    color_idx: usize,
    is_active: bool,
) {
    if ns >= ne {
        return;
    }
    let mut tmp = Vec::with_capacity(result.len() + 2);
    for (es, ee, ec, ea) in result.drain(..) {
        if ee <= ns || es >= ne {
            tmp.push((es, ee, ec, ea));
        } else {
            if es < ns {
                tmp.push((es, ns, ec, ea));
            }
            if ee > ne {
                tmp.push((ne, ee, ec, ea));
            }
        }
    }
    tmp.push((ns, ne, color_idx, is_active));
    tmp.sort_unstable_by_key(|&(s, _, _, _)| s);
    *result = tmp;
}

/// Collect substring paint ranges for one field (tag or msg); later patterns overwrite.
fn collect_field_matches(
    row: &EntryRow,
    patterns: &[PaintPattern<'_>],
    field: PaintField,
) -> Vec<ColoredMatch> {
    if patterns.is_empty() {
        return Vec::new();
    }
    let mut result: Vec<ColoredMatch> = Vec::new();
    for &(pattern, color_idx, is_active) in patterns {
        let spans = crate::fuzzy::map_search_positions(&row.tag, &row.msg, &row.raw, pattern);
        for sp in spans {
            let ok = match field {
                PaintField::Tag => sp.field == PaintField::Tag,
                PaintField::Msg => sp.field == PaintField::Msg || sp.field == PaintField::Raw,
                PaintField::Raw => sp.field == PaintField::Raw,
            };
            if ok {
                merge_colored_match(&mut result, sp.start, sp.end, color_idx, is_active);
            }
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

/// Choose tag column width: prefer `preferred_max`, shrink on narrow panes so
/// the message still gets at least 8 columns.
fn tag_col_for_area_max(
    area_width: usize,
    prefix_without_tag: usize,
    preferred_max: usize,
) -> usize {
    let reserved = prefix_without_tag + TAG_MSG_GAP + 8;
    let available = area_width.saturating_sub(reserved);
    if available == 0 {
        return 0;
    }
    preferred_max.min(available).max(TAG_COL_MIN.min(available))
}

/// Choose tag column width: prefer [`TAG_COL_WIDTH`], shrink on narrow panes so
/// the message still gets at least 8 columns.
pub(super) fn tag_col_for_area(area_width: usize, prefix_without_tag: usize) -> usize {
    tag_col_for_area_max(area_width, prefix_without_tag, TAG_COL_WIDTH)
}

/// Fit `tag` into a fixed display-column width: right-pad with spaces, or
/// truncate with `…`. Returns `(display, visible_byte_end)` where
/// `visible_byte_end` is the end of the prefix of `tag` shown before `…`
/// (equals `tag.len()` when not truncated).
pub(super) fn fit_tag_column(tag: &str, width: usize) -> (String, usize) {
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

/// Tag + message styles: accent/bold tag and default msg, or theme-red for
/// severe (E/F/crash) rows. Fatal and crash signatures are bold.
fn entry_text_styles(row: &EntryRow) -> (Style, Style) {
    if !row.severe {
        return (
            Style::default()
                .fg(theme::accent())
                .add_modifier(Modifier::BOLD),
            Style::default(),
        );
    }
    let emphatic = !matches!(row.level, alnav::parser::Level::E);
    let base = theme::severe_entry_style(emphatic);
    (base.add_modifier(Modifier::BOLD), base)
}

pub(super) fn render_entry_lines(
    row: &EntryRow,
    patterns: &[PaintPattern<'_>],
    area_width: usize,
    lineno: usize,
    lineno_width: usize,
) -> Vec<Line<'static>> {
    render_entry_lines_ex(
        row,
        patterns,
        area_width,
        Some((lineno, lineno_width)),
        false,
        false,
    )
}

pub(super) fn render_compare_entry_lines(
    row: &EntryRow,
    patterns: &[PaintPattern<'_>],
    area_width: usize,
) -> Vec<Line<'static>> {
    render_entry_lines_ex(row, patterns, area_width, None, true, false)
}

/// Renders one log entry as one or more physical `Line`s: a header
/// (optional lineno / timestamp / optional pid·tid / level / fixed tag column)
/// followed by the message, word-wrapped to `area_width`.
pub(super) fn render_entry_lines_ex(
    row: &EntryRow,
    patterns: &[PaintPattern<'_>],
    area_width: usize,
    lineno: Option<(usize, usize)>,
    show_pid_tid: bool,
    pinned: bool,
) -> Vec<Line<'static>> {
    let lineno_s = match lineno {
        Some((n, w)) if pinned => format!("{n:>w$}{}", theme::GLYPH_BOOKMARK_PIN),
        Some((n, w)) => format!("{n:>w$} "),
        None => String::new(),
    };
    let ts = format!("{} ", row.timestamp);
    let pid_s = if show_pid_tid && !row.pid.is_empty() {
        format!("{} ", row.pid)
    } else {
        String::new()
    };
    let tid_s = if show_pid_tid && !row.tid.is_empty() {
        format!("{} ", row.tid)
    } else {
        String::new()
    };
    let level_badge = format!(" {} ", row.level.as_char());
    let prefix_without_tag = lineno_s.chars().count()
        + ts.chars().count()
        + pid_s.chars().count()
        + tid_s.chars().count()
        + level_badge.chars().count()
        + LEVEL_TAG_GAP;
    let tag_col = tag_col_for_area(area_width, prefix_without_tag);
    let header_width = prefix_without_tag + tag_col + TAG_MSG_GAP;
    let cont_prefix: String = " ".repeat(header_width);

    let first_width = area_width.saturating_sub(header_width).max(8);
    let cont_width = area_width.saturating_sub(header_width).max(8);

    let (tag_style, msg_style) = entry_text_styles(row);
    let tag_matches = collect_field_matches(row, patterns, PaintField::Tag);
    let msg_matches = collect_field_matches(row, patterns, PaintField::Msg);

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
                if let Some((n, w)) = lineno {
                    spans.push(Span::styled(
                        format!("{n:>w$}"),
                        theme::muted().add_modifier(Modifier::DIM),
                    ));
                    if pinned {
                        spans.push(Span::styled(
                            theme::GLYPH_BOOKMARK_PIN.to_string(),
                            theme::bookmark_mark_style(),
                        ));
                    } else {
                        spans.push(Span::styled(
                            " ",
                            theme::muted().add_modifier(Modifier::DIM),
                        ));
                    }
                }
                spans.push(Span::styled(ts.clone(), theme::muted()));
                if !pid_s.is_empty() {
                    spans.push(Span::styled(pid_s.clone(), theme::muted()));
                }
                if !tid_s.is_empty() {
                    spans.push(Span::styled(tid_s.clone(), theme::muted()));
                }
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
            spans.extend(spans_for_range(&row.msg, range, &msg_matches, msg_style));
            Line::from(spans)
        })
        .collect()
}

/// Byte end of the first `max_chars` chars of `text` (or `text.len()` if shorter).
fn byte_end_for_chars(text: &str, max_chars: usize) -> usize {
    text.char_indices()
        .nth(max_chars)
        .map(|(i, _)| i)
        .unwrap_or(text.len())
}

/// Compact single-line Preview entry: timestamp/level/tag/msg colors like
/// LogList, but no lineno and a narrower tag column so msg keeps width.
pub(super) fn render_entry_line_single(
    row: &EntryRow,
    patterns: &[PaintPattern<'_>],
    area_width: usize,
) -> Line<'static> {
    let ts = format!("{} ", row.timestamp);
    let level_badge = format!(" {} ", row.level.as_char());
    let prefix_without_tag = ts.chars().count() + level_badge.chars().count() + LEVEL_TAG_GAP;
    let tag_col = tag_col_for_area_max(area_width, prefix_without_tag, PREVIEW_TAG_COL_MAX);
    let header_width = prefix_without_tag + tag_col + TAG_MSG_GAP;
    let msg_budget = area_width.saturating_sub(header_width).max(1);

    let (tag_style, msg_style) = entry_text_styles(row);
    let tag_matches = collect_field_matches(row, patterns, PaintField::Tag);
    let msg_matches = collect_field_matches(row, patterns, PaintField::Msg);

    let mut spans = Vec::new();
    spans.push(Span::styled(ts, theme::muted()));
    spans.push(Span::styled(
        level_badge,
        theme::level_badge_style(row.level),
    ));
    spans.push(Span::styled(" ".repeat(LEVEL_TAG_GAP), Style::default()));
    push_tag_column_spans(&mut spans, &row.tag, tag_col, &tag_matches, tag_style);
    spans.push(Span::styled(" ".repeat(TAG_MSG_GAP), Style::default()));

    let msg_chars = row.msg.chars().count();
    if msg_budget == 0 {
        return Line::from(spans);
    }
    if msg_chars <= msg_budget {
        spans.extend(spans_for_range(
            &row.msg,
            (0, row.msg.len()),
            &msg_matches,
            msg_style,
        ));
    } else if msg_budget == 1 {
        spans.push(Span::styled("…".to_string(), msg_style));
    } else {
        let visible_end = byte_end_for_chars(&row.msg, msg_budget - 1);
        spans.extend(spans_for_range(
            &row.msg,
            (0, visible_end),
            &msg_matches,
            msg_style,
        ));
        spans.push(Span::styled("…".to_string(), msg_style));
    }
    Line::from(spans)
}

/// Collapsed (single-line) LogList entry: same header layout as
/// `render_entry_lines` (lineno/timestamp/level/fixed tag column), but the
/// message is truncated with `…` instead of word-wrapped across multiple
/// `Line`s. Used when `App.collapsed_view` is toggled on (`w`).
pub(super) fn render_entry_line_collapsed(
    row: &EntryRow,
    patterns: &[PaintPattern<'_>],
    area_width: usize,
    lineno: usize,
    lineno_width: usize,
    pinned: bool,
) -> Line<'static> {
    let lineno_s = if pinned {
        format!("{lineno:>lineno_width$}{}", theme::GLYPH_BOOKMARK_PIN)
    } else {
        format!("{lineno:>lineno_width$} ")
    };
    let ts = format!("{} ", row.timestamp);
    let level_badge = format!(" {} ", row.level.as_char());
    let prefix_without_tag =
        lineno_s.chars().count() + ts.chars().count() + level_badge.chars().count() + LEVEL_TAG_GAP;
    let tag_col = tag_col_for_area(area_width, prefix_without_tag);
    let header_width = prefix_without_tag + tag_col + TAG_MSG_GAP;
    let msg_budget = area_width.saturating_sub(header_width).max(1);

    let (tag_style, msg_style) = entry_text_styles(row);
    let tag_matches = collect_field_matches(row, patterns, PaintField::Tag);
    let msg_matches = collect_field_matches(row, patterns, PaintField::Msg);

    let mut spans = Vec::new();
    spans.push(Span::styled(
        format!("{lineno:>lineno_width$}"),
        theme::muted().add_modifier(Modifier::DIM),
    ));
    if pinned {
        spans.push(Span::styled(
            theme::GLYPH_BOOKMARK_PIN.to_string(),
            theme::bookmark_mark_style(),
        ));
    } else {
        spans.push(Span::styled(
            " ",
            theme::muted().add_modifier(Modifier::DIM),
        ));
    }
    spans.push(Span::styled(ts, theme::muted()));
    spans.push(Span::styled(
        level_badge,
        theme::level_badge_style(row.level),
    ));
    spans.push(Span::styled(" ".repeat(LEVEL_TAG_GAP), Style::default()));
    push_tag_column_spans(&mut spans, &row.tag, tag_col, &tag_matches, tag_style);
    spans.push(Span::styled(" ".repeat(TAG_MSG_GAP), Style::default()));

    let msg_chars = row.msg.chars().count();
    if msg_chars <= msg_budget {
        spans.extend(spans_for_range(
            &row.msg,
            (0, row.msg.len()),
            &msg_matches,
            msg_style,
        ));
    } else if msg_budget == 1 {
        spans.push(Span::styled("…".to_string(), msg_style));
    } else {
        let visible_end = byte_end_for_chars(&row.msg, msg_budget - 1);
        spans.extend(spans_for_range(
            &row.msg,
            (0, visible_end),
            &msg_matches,
            msg_style,
        ));
        spans.push(Span::styled("…".to_string(), msg_style));
    }
    Line::from(spans)
}
