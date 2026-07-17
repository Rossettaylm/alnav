use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph};
use ratatui::Frame;
use regex::Regex;

use crate::app::{App, Focus, Mode};
use crate::input::InputBox;
use crate::model::EntryRow;
use crate::search_model::SearchBox;
use crate::theme;

fn rounded_block(title: Line<'static>, active: bool) -> Block<'static> {
    Block::new()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(theme::border_style(active))
        .title(title)
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

/// Match segment with its progressive highlight color index.
type ColoredMatch = (usize, usize, usize); // start, end, color_idx

/// Collect all pattern matches; later patterns overwrite overlapping ranges
/// (same order as `active_patterns`).
fn collect_matches(msg: &str, patterns: &[(&Regex, usize)]) -> Vec<ColoredMatch> {
    let mut marked: Vec<Option<usize>> = vec![None; msg.len()];
    for &(re, color_idx) in patterns {
        for m in re.find_iter(msg) {
            for i in m.start()..m.end() {
                marked[i] = Some(color_idx);
            }
        }
    }
    let mut out = Vec::new();
    let mut i = 0;
    while i < marked.len() {
        if let Some(color) = marked[i] {
            let start = i;
            i += 1;
            while i < marked.len() && marked[i] == Some(color) {
                i += 1;
            }
            // Only emit on char boundaries — marked is per-byte; regex matches
            // are already on char boundaries for UTF-8.
            out.push((start, i, color));
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
    for &(m_start, m_end, color_idx) in matches {
        if m_end <= start || m_start >= end {
            continue;
        }
        let seg_start = m_start.max(start);
        let seg_end = m_end.min(end);
        if seg_start > cursor {
            spans.push(Span::raw(msg[cursor..seg_start].to_string()));
        }
        spans.push(Span::styled(
            msg[seg_start..seg_end].to_string(),
            theme::highlight_style(color_idx),
        ));
        cursor = seg_end;
    }
    if cursor < end {
        spans.push(Span::raw(msg[cursor..end].to_string()));
    }
    spans
}

/// Renders one log entry as one or more physical `Line`s: a header
/// (timestamp/level/tag/pid:tid) followed by the message, word-wrapped to
/// `area_width` instead of being truncated. Fields use natural character
/// widths (no fixed column padding); continuation lines indent with spaces
/// matching the header width so the message column stays aligned.
fn render_entry_lines(row: &EntryRow, patterns: &[(&Regex, usize)], area_width: usize) -> Vec<Line<'static>> {
    let ts = format!("{} ", row.timestamp);
    let level_badge = format!(" {} ", row.level.as_char());
    let tag = format!("{} ", row.tag);
    let ids = format!("[{}:{}] ", row.pid, row.tid);
    let header_width = ts.chars().count() + level_badge.chars().count() + tag.chars().count() + ids.chars().count();
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
                spans.push(Span::styled(ts.clone(), theme::muted()));
                spans.push(Span::styled(level_badge.clone(), theme::level_badge_style(row.level)));
                spans.push(Span::styled(tag.clone(), Style::default().fg(theme::ACCENT).add_modifier(Modifier::BOLD)));
                spans.push(Span::styled(ids.clone(), theme::muted()));
            } else {
                spans.push(Span::styled(cont_prefix.clone(), Style::default().add_modifier(Modifier::DIM)));
            }
            spans.extend(spans_for_range(&row.msg, range, &matches));
            Line::from(spans)
        })
        .collect()
}

/// Shared chip-group strip rendering for Filter and Search strips.
fn render_group_strip_labels(
    labels: &[(String, bool)],
    cursor: usize,
    active: bool,
    empty_hint: &str,
) -> Vec<Span<'static>> {
    if labels.is_empty() {
        return vec![Span::styled(empty_hint.to_string(), Style::default().add_modifier(Modifier::DIM))];
    }
    labels
        .iter()
        .enumerate()
        .map(|(i, (label, enabled))| {
            let text = format!(" {label} ");
            if i == cursor && active {
                Span::styled(text, theme::focus_style())
            } else if !*enabled {
                Span::styled(text, theme::disabled_chip_style())
            } else {
                Span::raw(text)
            }
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
    let patterns = app.search_groups.active_patterns();

    let items: Vec<ListItem> = app
        .visible_rows()
        .enumerate()
        .map(|(i, row)| {
            let mut item = ListItem::new(render_entry_lines(row, &patterns, inner_width));
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

pub fn render_chip_strip(app: &App, frame: &mut Frame, area: Rect) {
    let active = app.focus == Focus::ChipStrip;
    let block = rounded_block(theme::numbered_title(1, "Filter", active), active);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let labels: Vec<(String, bool)> = app
        .groups
        .groups
        .iter()
        .map(|g| (g.label.clone(), g.enabled))
        .collect();
    let spans = render_group_strip_labels(&labels, app.group_cursor, active, "(no filter)");
    frame.render_widget(Paragraph::new(Line::from(spans)), inner);
}

pub fn render_search_chip_strip(app: &App, frame: &mut Frame, area: Rect) {
    let active = app.focus == Focus::SearchStrip;
    let block = rounded_block(theme::numbered_title(2, "Search", active), active);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let labels: Vec<(String, bool)> = app
        .search_groups
        .groups
        .iter()
        .map(|g| (g.label.clone(), g.enabled))
        .collect();
    let spans = render_group_strip_labels(&labels, app.search_cursor, active, "(no search)");
    frame.render_widget(Paragraph::new(Line::from(spans)), inner);
}

pub fn render_input_box(input: &InputBox, mode: Mode, focused: bool, frame: &mut Frame, area: Rect) {
    let block = rounded_block(theme::numbered_title(4, "Input", focused), focused);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut spans = vec![theme::mode_badge(mode), Span::raw(" ")];
    for chip in &input.chips {
        spans.push(Span::styled(
            format!("{}:", chip.field.keyword()),
            Style::default().fg(theme::field_color(chip.field)),
        ));
        spans.push(Span::raw(format!("{} ", chip.value)));
    }
    if let Some(field) = input.draft_field {
        spans.push(Span::styled(
            format!("{}:", field.keyword()),
            Style::default().fg(theme::field_color(field)),
        ));
    }
    spans.push(Span::raw(input.draft.clone()));
    if mode == Mode::Insert {
        spans.push(theme::caret(true));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), inner);
}

pub fn render_search_box(search: &SearchBox, frame: &mut Frame, area: Rect) {
    let editing = search.editing;
    let block = rounded_block(theme::plain_title("Search", editing), editing);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut spans = Vec::new();
    if editing || !search.is_empty() {
        spans.push(Span::styled("/", Style::default().fg(theme::ACCENT).add_modifier(Modifier::BOLD)));
        for chip in &search.chips {
            spans.push(Span::styled(
                format!("[{chip}] "),
                Style::default().fg(theme::WARNING),
            ));
        }
        spans.push(Span::raw(search.draft.clone()));
        if editing {
            spans.push(Span::raw(" "));
            spans.push(theme::caret(true));
        }
    } else {
        spans.push(Span::styled("(no search)", Style::default().add_modifier(Modifier::DIM)));
        spans.push(Span::raw(" "));
        spans.push(theme::caret(false));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), inner);
}

pub fn render_popup(input: &InputBox, frame: &mut Frame, area: Rect) {
    let Some(popup) = &input.popup else { return };
    frame.render_widget(Clear, area);

    let matches = popup.matches();
    let block = rounded_block(theme::plain_title("字段", true), true);

    if matches.is_empty() {
        let inner = block.inner(area);
        frame.render_widget(block, area);
        frame.render_widget(
            Paragraph::new(Span::styled("无匹配字段", Style::default().add_modifier(Modifier::DIM))),
            inner,
        );
        return;
    }

    let items: Vec<ListItem> = matches
        .iter()
        .map(|&f| {
            ListItem::new(Span::styled(format!(" {} ", f.keyword()), Style::default().fg(theme::field_color(f))))
        })
        .collect();
    let list = List::new(items).block(block).highlight_style(theme::focus_style()).highlight_symbol("\u{203a} ");
    let mut state = ListState::default();
    state.select(Some(popup.selected));
    frame.render_stateful_widget(list, area, &mut state);
}

pub fn render_status_bar(app: &App, frame: &mut Frame, area: Rect) {
    let mut spans = vec![Span::styled(
        format!("{}/{}", app.cursor + 1, app.visible.len()),
        Style::default().add_modifier(Modifier::DIM),
    )];
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
            .push(SearchGroup::from_patterns(&["error".into()]).unwrap());
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
        let lines = render_entry_lines(&row, &[], 40);
        assert!(lines.len() > 1, "a long message should wrap into multiple lines, got {}", lines.len());
    }

    #[test]
    fn test_render_entry_lines_highlights_only_matched_keyword() {
        let row = EntryRow::from_line("04-02 10:00:00.000  1  1 I Tag     : an error occurred here").unwrap();
        let re = Regex::new("(?i)error").unwrap();
        let patterns = [(&re, 0usize)];
        let lines = render_entry_lines(&row, &patterns, 200);
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
        let patterns = [(&re0, 0usize), (&re1, 1usize)];
        let lines = render_entry_lines(&row, &patterns, 200);
        let foo = lines[0].spans.iter().find(|s| s.content.as_ref() == "foo").unwrap();
        let bar = lines[0].spans.iter().find(|s| s.content.as_ref() == "bar").unwrap();
        assert_eq!(foo.style, theme::highlight_style(0));
        assert_eq!(bar.style, theme::highlight_style(1));
        assert_ne!(foo.style.bg, bar.style.bg);
    }

    #[test]
    fn test_render_entry_lines_uses_natural_tag_width_no_fixed_padding() {
        let row = EntryRow::from_line("04-02 10:00:00.000  1  1 I Ab   : msg").unwrap();
        let lines = render_entry_lines(&row, &[], 200);
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
        let lines = render_entry_lines(&row, &[], 40);
        assert!(lines.len() > 1);
        let header_width: usize = lines[0].spans.iter().take(4).map(|s| s.content.chars().count()).sum();
        let cont = lines[1].spans[0].content.as_ref();
        assert!(cont.chars().all(|c| c == ' '), "continuation prefix should be spaces");
        assert_eq!(cont.chars().count(), header_width);
    }
}
