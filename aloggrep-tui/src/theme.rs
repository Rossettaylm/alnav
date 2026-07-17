//! Single source of truth for aloggrep-tui's color mapping (see CLAUDE.md
//! "UI 设计指导" for the design rules this module implements). Log-severity
//! and highlight colors are derived from `aloggrep::logcolor` so the TUI's
//! ratatui rendering stays visually in sync with the CLI's ANSI text output.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use aloggrep::logcolor::{self, Badge};
use aloggrep::parser::Level;

use crate::app::Mode;
use crate::input::ChipField;

pub const ACCENT: Color = Color::Cyan;
pub const SUCCESS: Color = Color::Green;
pub const WARNING: Color = Color::Yellow;

fn rgb((r, g, b): logcolor::Rgb) -> Color {
    Color::Rgb(r, g, b)
}

/// Timestamp/pid/tid/separator tint, shared with the CLI's muted gray.
pub fn muted() -> Style {
    Style::default().fg(rgb(logcolor::MUTED))
}

/// Colored level badge (e.g. `" E "` on a red background), mirroring the
/// CLI's `formatter::level_badge`.
pub fn level_badge_style(level: Level) -> Style {
    match logcolor::level_badge(level) {
        Badge::Gray => Style::default().fg(Color::White).bg(rgb(logcolor::VERBOSE_BG)),
        Badge::Blue => Style::default().fg(Color::Black).bg(Color::Blue),
        Badge::Green => Style::default().fg(Color::Black).bg(Color::Green),
        Badge::Yellow => Style::default().fg(Color::Black).bg(Color::Yellow),
        Badge::Red => Style::default().fg(Color::White).bg(Color::Red),
        Badge::RedBold => Style::default().fg(Color::White).bg(Color::Red).add_modifier(Modifier::BOLD),
    }
}

/// One of the 8 reading-friendly highlight-palette colors, cycled by index.
/// TUI search chips assign a progressive index per pattern; CLI `--highlight`
/// does the same.
pub fn highlight_style(idx: usize) -> Style {
    let ((r, g, b), fg_black) = logcolor::USER_HIGHLIGHT[idx % logcolor::USER_HIGHLIGHT.len()];
    let fg = if fg_black { Color::Black } else { Color::White };
    Style::default().fg(fg).bg(Color::Rgb(r, g, b)).add_modifier(Modifier::BOLD)
}

/// Soft-disabled chip/group label (`di`): dim gray, distinct from focus and
/// from normal labels.
pub fn disabled_chip_style() -> Style {
    Style::default().fg(Color::DarkGray).add_modifier(Modifier::DIM)
}

/// Chip field -> accent color, shared by the input box, popup, and (once
/// committed) the chip strip so a field always reads the same color
/// everywhere it appears.
pub fn field_color(field: ChipField) -> Color {
    match field {
        ChipField::Tag => ACCENT,
        ChipField::Msg => SUCCESS,
        ChipField::Pkg => Color::LightYellow,
        ChipField::Pid => Color::Magenta,
        ChipField::Tid => Color::LightMagenta,
        ChipField::Level => WARNING,
    }
}

/// Reverse-color block used for whatever currently has keyboard focus
/// (selected chip, selected popup entry) instead of border/bold color
/// changes.
pub fn focus_style() -> Style {
    Style::default().fg(Color::Black).bg(ACCENT).add_modifier(Modifier::BOLD)
}

/// Mode badge shown at the start of the input line.
pub fn mode_badge(mode: Mode) -> Span<'static> {
    match mode {
        Mode::Normal => Span::styled(" NORMAL ", Style::default().add_modifier(Modifier::DIM)),
        Mode::Insert => Span::styled(" INSERT ", focus_style()),
    }
}

/// Border color for a bordered region: bright accent when it currently has
/// keyboard focus, dim gray otherwise (terminals have no real alpha
/// channel, so `DIM` stands in for "reduced opacity").
pub fn border_style(active: bool) -> Style {
    if active {
        Style::default().fg(ACCENT)
    } else {
        Style::default().fg(Color::DarkGray).add_modifier(Modifier::DIM)
    }
}

/// Border title for a numbered, Tab-cyclable region (Filter/Search/Log/Input):
/// a digit badge followed by a label, both styled by whether the region is
/// currently focused.
pub fn numbered_title(number: u8, label: &str, active: bool) -> Line<'static> {
    let badge_style = if active { focus_style() } else { Style::default().add_modifier(Modifier::DIM) };
    let label_style = if active {
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
    } else {
        Style::default().add_modifier(Modifier::DIM)
    };
    Line::from(vec![
        Span::styled(format!(" {number} "), badge_style),
        Span::styled(format!(" {label} "), label_style),
    ])
}

/// Border title for a region that isn't part of the numbered Tab cycle
/// (the search box, the field popup).
pub fn plain_title(label: &str, active: bool) -> Line<'static> {
    let label_style = if active {
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
    } else {
        Style::default().add_modifier(Modifier::DIM)
    };
    Line::from(Span::styled(format!(" {label} "), label_style))
}

/// Fake caret appended after drafted text: a block for the "idle/normal"
/// state, a thin line for "actively editing" (mirrors vim's Normal/Insert
/// cursor shapes).
pub fn caret(is_editing: bool) -> Span<'static> {
    if is_editing {
        Span::styled("▏", Style::default().fg(ACCENT).add_modifier(Modifier::BOLD))
    } else {
        Span::styled("█", Style::default().fg(ACCENT))
    }
}

/// Selected-row background in the log list — a quiet, low-contrast gray
/// instead of a full reverse-video block. Applied via `ListItem::style` (not
/// `List::highlight_style`) so keyword highlight spans keep their own bg;
/// see `ui::render_log_list`.
pub fn log_selection_style() -> Style {
    Style::default().bg(Color::DarkGray)
}

/// Background for rows inside a visual-line selection (`V` … `y`). Distinct
/// from `log_selection_style` so the range reads as a block, not a single
/// cursor highlight.
pub fn log_visual_style() -> Style {
    Style::default().bg(Color::Rgb(30, 60, 70))
}

/// Reverse badge used for transient status hints (FOLLOWING / YANKED / VISUAL).
pub fn status_badge(label: &str, bg: Color) -> Span<'static> {
    Span::styled(
        format!(" {label} "),
        Style::default().fg(Color::Black).bg(bg).add_modifier(Modifier::BOLD),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_selection_style_is_soft_gray_no_reverse() {
        let style = log_selection_style();
        assert_eq!(style.bg, Some(Color::DarkGray));
        assert!(!style.add_modifier.contains(Modifier::REVERSED));
    }

    #[test]
    fn test_log_visual_style_differs_from_selection() {
        assert_ne!(log_visual_style().bg, log_selection_style().bg);
    }
}
