//! Editable text windowing and display-width padding / truncation helpers.

use ratatui::style::Style;
use ratatui::text::Span;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Display-column width of styled spans (sum of content widths).
pub(super) fn spans_display_width(spans: &[Span<'_>]) -> usize {
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

/// Pad/truncate `s` to a fixed display width (Unicode-width aware).
pub(super) fn pad_display(s: &str, width: usize) -> String {
    let mut out = String::new();
    let mut w = 0usize;
    for c in s.chars() {
        let cw = UnicodeWidthChar::width(c).unwrap_or(0);
        if w + cw > width {
            break;
        }
        out.push(c);
        w += cw;
    }
    while w < width {
        out.push(' ');
        w += 1;
    }
    out
}

pub(super) fn fit_display_end(text: &str, max_width: usize) -> String {
    if UnicodeWidthStr::width(text) <= max_width {
        return text.to_string();
    }
    if max_width == 0 {
        return String::new();
    }
    if max_width == 1 {
        return "…".to_string();
    }
    let mut out = take_display_prefix(text, max_width - 1);
    out.push('…');
    out
}

pub(super) fn fit_display_middle(text: &str, max_width: usize) -> String {
    if UnicodeWidthStr::width(text) <= max_width {
        return text.to_string();
    }
    if max_width == 0 {
        return String::new();
    }
    if max_width == 1 {
        return "…".to_string();
    }
    let left_width = (max_width - 1) / 2;
    let right_width = max_width - 1 - left_width;
    format!(
        "{}…{}",
        take_display_prefix(text, left_width),
        take_display_suffix(text, right_width)
    )
}

fn take_display_prefix(text: &str, max_width: usize) -> String {
    let mut used = 0;
    text.graphemes(true)
        .take_while(|grapheme| {
            let width = UnicodeWidthStr::width(*grapheme);
            let fits = used + width <= max_width;
            if fits {
                used += width;
            }
            fits
        })
        .collect::<String>()
}

fn take_display_suffix(text: &str, max_width: usize) -> String {
    let mut used = 0;
    let mut graphemes: Vec<&str> = text
        .graphemes(true)
        .rev()
        .take_while(|grapheme| {
            let width = UnicodeWidthStr::width(*grapheme);
            let fits = used + width <= max_width;
            if fits {
                used += width;
            }
            fits
        })
        .collect();
    graphemes.reverse();
    graphemes.concat()
}
