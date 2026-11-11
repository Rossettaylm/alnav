//! Entry row painting and wrap tests.

use crate::model::EntryRow;
use crate::theme;
use crate::ui::entry::{
    fit_tag_column, render_entry_line_collapsed, render_entry_line_single, render_entry_lines,
    tag_col_for_area, wrap_ranges, LEVEL_TAG_GAP, TAG_COL_WIDTH, TAG_MSG_GAP,
};
use ratatui::style::{Modifier, Style};
use ratatui::text::Span;
use unicode_width::UnicodeWidthStr;

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
fn test_render_entry_line_single_truncates_msg_with_ellipsis() {
    let long = "word ".repeat(40);
    let line = format!("04-02 10:00:00.000  1  1 I Tag     : {long}");
    let row = EntryRow::from_line(&line).unwrap();
    let rendered = render_entry_line_single(&row, &[], 60);
    let text: String = rendered.spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(
        text.contains('…'),
        "single-line preview must ellipsize: {text}"
    );
    assert!(
        !text.contains('\n'),
        "single-line preview must stay one physical line"
    );
    assert!(
        text.starts_with("04-02 10:00:00.000"),
        "must start at timestamp"
    );
}

#[test]
fn test_render_entry_line_single_highlights_pattern() {
    let row =
        EntryRow::from_line("04-02 10:00:00.000  1  1 I Tag     : an error occurred").unwrap();
    let patterns = [("error", 0usize, true)];
    let rendered = render_entry_line_single(&row, &patterns, 200);
    let matched = rendered
        .spans
        .iter()
        .find(|s| s.content.as_ref() == "error")
        .expect("highlight span");
    assert_eq!(matched.style, theme::highlight_style_active(0));
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
        EntryRow::from_line("04-02 10:00:00.000  1  1 I Tag     : an error occurred here").unwrap();
    let patterns = [("error", 0usize, false)];
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
    let patterns = [("tag", 0usize, false)];
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
    let row = EntryRow::from_line("04-02 10:00:00.000  1  1 I Tag     : foo and bar here").unwrap();
    let patterns = [("foo", 0usize, true), ("bar", 1usize, false)];
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
    let row = EntryRow::from_line(&format!("04-02 10:00:00.000  1  1 I {long}   : msg")).unwrap();
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
fn test_render_entry_line_collapsed_truncates_msg_with_ellipsis() {
    let long = "word ".repeat(40);
    let line = format!("04-02 10:00:00.000  1  1 I Tag     : {long}");
    let row = EntryRow::from_line(&line).unwrap();
    let rendered = render_entry_line_collapsed(&row, &[], 60, 12, 3, false);
    let text: String = rendered.spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(text.contains('…'), "collapsed line must ellipsize: {text}");
    assert!(
        !text.contains('\n'),
        "collapsed line must stay a single physical line"
    );
    assert!(
        text.contains(" 12 "),
        "collapsed line must keep the lineno prefix like render_entry_lines"
    );
    assert!(
        text.starts_with(" 12 04-02 10:00:00.000"),
        "must start with lineno then timestamp"
    );
}

#[test]
fn test_render_entry_line_collapsed_keeps_short_msg_untouched() {
    let row = EntryRow::from_line("04-02 10:00:00.000  1  1 I MyTag   : hello").unwrap();
    let rendered = render_entry_line_collapsed(&row, &[], 200, 1, 1, false);
    let text: String = rendered.spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(!text.contains('…'), "short msg must not be truncated");
    assert!(text.contains("MyTag"));
    assert!(text.contains("hello"));
}

#[test]
fn test_render_entry_line_collapsed_highlights_pattern() {
    let row =
        EntryRow::from_line("04-02 10:00:00.000  1  1 I Tag     : an error occurred").unwrap();
    let patterns = [("error", 0usize, true)];
    let rendered = render_entry_line_collapsed(&row, &patterns, 200, 1, 1, false);
    let matched = rendered
        .spans
        .iter()
        .find(|s| s.content.as_ref() == "error")
        .expect("highlight span");
    assert_eq!(matched.style, theme::highlight_style_active(0));
}
