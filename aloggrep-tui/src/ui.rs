use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph};
use ratatui::Frame;
use regex::Regex;

use crate::app::{App, Focus, Mode};
use crate::input::InputBox;
use crate::model::EntryRow;
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

/// Splits `msg[range.0..range.1]` into plain/highlighted spans, given
/// match byte-ranges already computed against the *full* `msg` (so this
/// composes with `wrap_ranges` cutting the same string into physical
/// lines without the two disagreeing on byte offsets).
fn spans_for_range(msg: &str, range: (usize, usize), matches: &[(usize, usize)]) -> Vec<Span<'static>> {
    let (start, end) = range;
    let mut spans = Vec::new();
    let mut cursor = start;
    for &(m_start, m_end) in matches {
        if m_end <= start || m_start >= end {
            continue;
        }
        let seg_start = m_start.max(start);
        let seg_end = m_end.min(end);
        if seg_start > cursor {
            spans.push(Span::raw(msg[cursor..seg_start].to_string()));
        }
        spans.push(Span::styled(msg[seg_start..seg_end].to_string(), theme::highlight_style(0)));
        cursor = seg_end;
    }
    if cursor < end {
        spans.push(Span::raw(msg[cursor..end].to_string()));
    }
    spans
}

const CONT_PREFIX: &str = "        ";

/// Renders one log entry as one or more physical `Line`s: a header
/// (timestamp/level/tag/pid:tid) followed by the message, word-wrapped to
/// `area_width` instead of being truncated. Continuation lines get a dim
/// indent instead of repeating the header.
fn render_entry_lines(row: &EntryRow, highlight: &Option<Regex>, area_width: usize) -> Vec<Line<'static>> {
    let ts = format!("{:<18} ", row.timestamp);
    let level_badge = format!(" {} ", row.level.as_char());
    let tag = format!("{:<16} ", row.tag);
    let ids = format!("[{}:{}] ", row.pid, row.tid);
    let header_width = ts.chars().count() + level_badge.chars().count() + tag.chars().count() + ids.chars().count();

    let first_width = area_width.saturating_sub(header_width).max(8);
    let cont_width = area_width.saturating_sub(CONT_PREFIX.len()).max(8);

    let matches: Vec<(usize, usize)> = match highlight {
        Some(re) => re.find_iter(&row.msg).map(|m| (m.start(), m.end())).collect(),
        None => Vec::new(),
    };

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
                spans.push(Span::styled(CONT_PREFIX, Style::default().add_modifier(Modifier::DIM)));
            }
            spans.extend(spans_for_range(&row.msg, range, &matches));
            Line::from(spans)
        })
        .collect()
}

pub fn render_log_list(app: &App, frame: &mut Frame, area: Rect) {
    let active = app.focus == Focus::LogList;
    let block = rounded_block(theme::numbered_title(2, "Log", active), active);
    let inner_width = block.inner(area).width.max(1) as usize;

    let items: Vec<ListItem> = app
        .visible_rows()
        .map(|row| ListItem::new(render_entry_lines(row, &app.highlight, inner_width)))
        .collect();
    let list = List::new(items).block(block).highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    let mut state = ListState::default();
    if !app.visible.is_empty() {
        state.select(Some(app.cursor));
    }
    frame.render_stateful_widget(list, area, &mut state);
}

pub fn render_chip_strip(app: &App, frame: &mut Frame, area: Rect) {
    let active = app.focus == Focus::ChipStrip;
    let block = rounded_block(theme::numbered_title(1, "Filter", active), active);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let spans: Vec<Span> = if app.groups.groups.is_empty() {
        vec![Span::styled("(no filter)", Style::default().add_modifier(Modifier::DIM))]
    } else {
        app.groups
            .groups
            .iter()
            .enumerate()
            .map(|(i, g)| {
                let label = format!(" {} ", g.label);
                if i == app.group_cursor && active {
                    Span::styled(label, theme::focus_style())
                } else {
                    Span::raw(label)
                }
            })
            .collect()
    };
    frame.render_widget(Paragraph::new(Line::from(spans)), inner);
}

pub fn render_input_box(input: &InputBox, mode: Mode, focused: bool, frame: &mut Frame, area: Rect) {
    let block = rounded_block(theme::numbered_title(3, "Input", focused), focused);
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

pub fn render_search_box(app: &App, frame: &mut Frame, area: Rect) {
    let editing = app.search_draft.is_some();
    let block = rounded_block(theme::plain_title("Search", editing), editing);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut spans = Vec::new();
    if let Some(draft) = &app.search_draft {
        spans.push(Span::styled("/", Style::default().fg(theme::ACCENT).add_modifier(Modifier::BOLD)));
        spans.push(Span::raw(draft.clone()));
        spans.push(Span::raw(" "));
        spans.push(theme::caret(true));
    } else {
        let pattern = app
            .highlight
            .as_ref()
            .map(|re| format!("/{}", re.as_str()))
            .unwrap_or_else(|| "(no highlight)".to_string());
        spans.push(Span::styled(pattern, Style::default().add_modifier(Modifier::DIM)));
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
        spans.push(Span::styled(
            " FOLLOWING ",
            Style::default().fg(Color::Black).bg(theme::SUCCESS).add_modifier(Modifier::BOLD),
        ));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

#[cfg(test)]
mod tests {
    use super::*;
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
            .draw(|frame| render_log_list(&app, frame, frame.area()))
            .unwrap();

        let content = cell_text(terminal.backend().buffer());
        assert!(content.contains("MyTag"));
        assert!(content.contains("hello world"));
    }

    #[test]
    fn test_render_log_list_highlights_selected_row() {
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
            .draw(|frame| render_log_list(&app, frame, frame.area()))
            .unwrap();

        // Row 0 is the block's top border; content starts at y=1 (one row
        // per entry, no wrapping for these short messages) and x=1 (past
        // the left border column).
        let buf = terminal.backend().buffer();
        let selected_style = buf[(1, 2)].style();
        let unselected_style = buf[(1, 1)].style();
        assert_ne!(
            selected_style, unselected_style,
            "selected row (y=2) should be styled differently from the unselected row (y=1)"
        );
        assert!(selected_style.add_modifier.contains(Modifier::REVERSED));
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
        let lines = render_entry_lines(&row, &None, 40);
        assert!(lines.len() > 1, "a long message should wrap into multiple lines, got {}", lines.len());
    }

    #[test]
    fn test_render_entry_lines_highlights_only_matched_keyword() {
        let row = EntryRow::from_line("04-02 10:00:00.000  1  1 I Tag     : an error occurred here").unwrap();
        let highlight = Some(Regex::new("error").unwrap());
        let lines = render_entry_lines(&row, &highlight, 200);
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
}
