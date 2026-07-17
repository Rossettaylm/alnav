//! Single source of truth for aloggrep-tui's color mapping (see CLAUDE.md
//! "UI 设计指导" for the design rules this module implements). Log-severity
//! and highlight colors are derived from `aloggrep::logcolor` so the TUI's
//! ratatui rendering stays visually in sync with the CLI's ANSI text output.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use aloggrep::logcolor::{self, Badge};
use aloggrep::parser::Level;

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

/// Status-bar search hit counter `[k/N]`: accent foreground only (no reverse
/// badge), so it reads as related-but-distinct from the dim filter `cursor/total`.
pub fn search_match_status_style() -> Style {
    Style::default().fg(ACCENT)
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
/// (selected popup entry, numbered title badge) instead of border/bold color
/// changes. Not used for Filter/Search chip selection — those tint the
/// group `●`/`○` via [`chip_group_border_style`].
pub fn focus_style() -> Style {
    Style::default().fg(Color::Black).bg(ACCENT).add_modifier(Modifier::BOLD)
}

/// Chip-group selection accent: Magenta, distinct from focused-region [`ACCENT`].
pub const SELECTION_FRAME: Color = Color::Magenta;

/// Style for the group `●`/`○` marker (selected = Magenta, else dim).
/// One cell wide so chip strips stay a single content row tall.
pub fn chip_group_border_style(selected: bool) -> Style {
    if selected {
        Style::default().fg(SELECTION_FRAME)
    } else {
        Style::default().fg(Color::DarkGray).add_modifier(Modifier::DIM)
    }
}

/// Body style for a filter pill (text + fg/bg fill). Drawn as a single-row
/// filled span inside the strip's rounded region border (per-chip `Block`
/// chrome needs 3 rows and doubles strip height on typical cell aspect ratios).
pub fn chip_pill_style(field: ChipField, value: &str, disabled: bool) -> (String, Style) {
    let text = format!(" {value} ");
    if disabled {
        return (text, disabled_chip_style());
    }
    let style = match field {
        ChipField::Level => {
            let level = match value.chars().next().unwrap_or('I').to_ascii_uppercase() {
                'V' => Level::V,
                'D' => Level::D,
                'I' => Level::I,
                'W' => Level::W,
                'E' => Level::E,
                'F' => Level::F,
                _ => Level::I,
            };
            level_badge_style(level)
        }
        other => Style::default()
            .fg(Color::Black)
            .bg(field_color(other))
            .add_modifier(Modifier::BOLD),
    };
    (text, style)
}

/// Body style for a search pill — same single-row fill model as [`chip_pill_style`].
pub fn search_pill_style(pattern: &str, color_idx: usize, disabled: bool) -> (String, Style) {
    let text = format!(" {pattern} ");
    if disabled {
        return (text, disabled_chip_style());
    }
    (text, highlight_style(color_idx))
}

/// Thin editing caret (Insert / search editing). Idle block caret is unused.
///
/// Uses [`Style::reset`] so a preceding filled pill cannot leave its `bg` on
/// the caret cell (`Cell::set_style` only applies `Some(_)` fields).
pub fn caret_bar() -> Span<'static> {
    Span::styled(
        "▏",
        Style::reset().fg(ACCENT).add_modifier(Modifier::BOLD),
    )
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

    #[test]
    fn test_chip_group_border_style_distinct_from_region_accent() {
        assert_ne!(SELECTION_FRAME, ACCENT);
        assert_eq!(chip_group_border_style(true).fg, Some(SELECTION_FRAME));
        assert_eq!(chip_group_border_style(false).fg, Some(Color::DarkGray));
    }

    #[test]
    fn test_chip_pill_style_fill() {
        let (text, body) = chip_pill_style(ChipField::Tag, "MyTag", false);
        assert!(text.contains("MyTag"));
        assert_eq!(body.bg, Some(ACCENT));
    }

    #[test]
    fn test_caret_bar_resets_background() {
        let caret = caret_bar();
        assert_eq!(caret.content.as_ref(), "▏");
        assert_eq!(caret.style.fg, Some(ACCENT));
        assert_eq!(
            caret.style.bg,
            Some(Color::Reset),
            "caret must clear pill bg via Style::reset"
        );
    }
}
