//! Single source of truth for aloggrep-tui's color mapping (see CLAUDE.md
//! "UI 设计指导" for the design rules this module implements). Log-severity
//! and highlight colors are derived from `aloggrep::logcolor` so the TUI's
//! ratatui rendering stays visually in sync with the CLI's ANSI text output.
//!
//! UI chrome tokens (accent, selection, preview, …) may be overridden at
//! startup via `theme.toml` (M4). Log colors are **never** loaded from that file.

use std::sync::Mutex;

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use serde::Deserialize;

use aloggrep::logcolor::{self, Badge};
use aloggrep::parser::Level;

use crate::input::ChipField;

/// Overridable UI chrome tokens (not log severity / USER_HIGHLIGHT).
#[derive(Debug, Clone, PartialEq)]
pub struct UiTokens {
    pub accent: Color,
    pub success: Color,
    pub warning: Color,
    pub lock: Color,
    pub selection_frame: Color,
    pub log_selection_bg: Color,
    pub log_visual_bg: Color,
    pub preview_highlight_bg: Color,
    pub border_inactive: Color,
    pub candidate_selection_bg: Color,
    pub bookmark_strip_bg: Color,
}

impl UiTokens {
    pub fn builtin() -> Self {
        Self {
            accent: Color::Cyan,
            success: Color::Green,
            warning: Color::Yellow,
            lock: Color::Magenta,
            selection_frame: Color::Magenta,
            log_selection_bg: Color::DarkGray,
            log_visual_bg: Color::Rgb(30, 60, 70),
            preview_highlight_bg: Color::DarkGray,
            border_inactive: Color::DarkGray,
            candidate_selection_bg: Color::DarkGray,
            bookmark_strip_bg: Color::DarkGray,
        }
    }
}

static TOKENS: Mutex<Option<UiTokens>> = Mutex::new(None);

/// Install tokens for the process (startup / tests).
pub fn install(tokens: UiTokens) {
    *TOKENS.lock().expect("theme lock") = Some(tokens);
}

fn t() -> UiTokens {
    TOKENS
        .lock()
        .expect("theme lock")
        .clone()
        .unwrap_or_else(UiTokens::builtin)
}

pub fn accent() -> Color {
    t().accent
}
pub fn success() -> Color {
    t().success
}
pub fn warning() -> Color {
    t().warning
}
pub fn lock() -> Color {
    t().lock
}
pub fn selection_frame() -> Color {
    t().selection_frame
}

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
        Badge::Gray => Style::default()
            .fg(Color::White)
            .bg(rgb(logcolor::VERBOSE_BG)),
        Badge::Blue => Style::default().fg(Color::Black).bg(Color::Blue),
        Badge::Green => Style::default().fg(Color::Black).bg(Color::Green),
        Badge::Yellow => Style::default().fg(Color::Black).bg(Color::Yellow),
        Badge::Red => Style::default().fg(Color::White).bg(Color::Red),
        Badge::RedBold => Style::default()
            .fg(Color::White)
            .bg(Color::Red)
            .add_modifier(Modifier::BOLD),
    }
}

/// One of the 8 reading-friendly highlight-palette colors, cycled by index.
/// TUI search chips assign a progressive index per pattern; CLI `--highlight`
/// does the same. **Not** overridable via theme.toml.
pub fn highlight_style(idx: usize) -> Style {
    let ((r, g, b), fg_black) = logcolor::USER_HIGHLIGHT[idx % logcolor::USER_HIGHLIGHT.len()];
    let fg = if fg_black { Color::Black } else { Color::White };
    Style::default()
        .fg(fg)
        .bg(Color::Rgb(r, g, b))
        .add_modifier(Modifier::BOLD)
}

/// [`highlight_style`] plus underline for the globally active search pattern.
pub fn highlight_style_active(idx: usize) -> Style {
    highlight_style(idx).add_modifier(Modifier::UNDERLINED)
}

/// Soft-disabled chip/group label (`di`): dim gray, distinct from focus and
/// from normal labels.
pub fn disabled_chip_style() -> Style {
    Style::default()
        .fg(t().border_inactive)
        .add_modifier(Modifier::DIM)
}

/// Status-bar search hit counter `[k/N]`: accent foreground only (no reverse
/// badge), so it reads as related-but-distinct from the dim filter `cursor/total`.
pub fn search_match_status_style() -> Style {
    Style::default().fg(accent())
}

/// Chip field -> accent color, shared by the input box, popup, and (once
/// committed) the chip strip so a field always reads the same color
/// everywhere it appears.
pub fn field_color(field: ChipField) -> Color {
    match field {
        ChipField::Tag => accent(),
        ChipField::Msg => success(),
        ChipField::Pkg => Color::LightYellow,
        ChipField::Pid => Color::Magenta,
        ChipField::Tid => Color::LightMagenta,
        ChipField::Level => warning(),
    }
}

/// Reverse-color block used for numbered title badges and other high-emphasis
/// focus chrome. Candidate list selection uses [`candidate_selection_style`]
/// instead (softer). Not used for Filter/Search chip selection — those tint
/// the group `●`/`○` via [`chip_group_border_style`].
pub fn focus_style() -> Style {
    Style::default()
        .fg(Color::Black)
        .bg(accent())
        .add_modifier(Modifier::BOLD)
}

/// Soft gray highlight for Input/Search candidate list selection — same quiet
/// wash as [`log_selection_style`], with white fg so colored candidate labels
/// stay readable (List::highlight_style patches over item fg).
pub fn candidate_selection_style() -> Style {
    Style::default()
        .fg(Color::White)
        .bg(t().candidate_selection_bg)
}

/// Style for the group `●`/`○` marker (selected = selection_frame, else dim).
/// One cell wide so chip strips stay a single content row tall.
pub fn chip_group_border_style(selected: bool) -> Style {
    if selected {
        Style::default().fg(selection_frame())
    } else {
        Style::default()
            .fg(t().border_inactive)
            .add_modifier(Modifier::DIM)
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

/// Exclude pill (H9): same fill as [`chip_pill_style`] but with a `!` prefix.
pub fn exclude_pill_style(field: ChipField, value: &str, disabled: bool) -> (String, Style) {
    let (inner, style) = chip_pill_style(field, value, disabled);
    (format!("!{inner}"), style)
}

/// Body style for a search pill — same single-row fill model as [`chip_pill_style`].
/// `active_global` underlines the globally active (n/N) search chip.
pub fn search_pill_style(
    pattern: &str,
    color_idx: usize,
    disabled: bool,
    active_global: bool,
) -> (String, Style) {
    let text = format!(" {pattern} ");
    if disabled {
        return (text, disabled_chip_style());
    }
    let style = if active_global {
        highlight_style_active(color_idx)
    } else {
        highlight_style(color_idx)
    };
    (text, style)
}

/// Thin editing caret (Insert / search editing). Idle block caret is unused.
///
/// Uses [`Style::reset`] so a preceding filled pill cannot leave its `bg` on
/// the caret cell (`Cell::set_style` only applies `Some(_)` fields).
pub fn caret_bar() -> Span<'static> {
    Span::styled(
        "▏",
        Style::reset().fg(accent()).add_modifier(Modifier::BOLD),
    )
}

/// Border color for a bordered region: bright accent when it currently has
/// keyboard focus, dim gray otherwise (terminals have no real alpha
/// channel, so `DIM` stands in for "reduced opacity").
pub fn border_style(active: bool) -> Style {
    if active {
        Style::default().fg(accent())
    } else {
        Style::default()
            .fg(t().border_inactive)
            .add_modifier(Modifier::DIM)
    }
}

/// Border title for a numbered, Tab-cyclable region (Filter/Search/Log/Input):
/// a digit badge followed by a label, both styled by whether the region is
/// currently focused.
pub fn numbered_title(number: u8, label: &str, active: bool) -> Line<'static> {
    let badge_style = if active {
        focus_style()
    } else {
        Style::default().add_modifier(Modifier::DIM)
    };
    let label_style = if active {
        Style::default().fg(accent()).add_modifier(Modifier::BOLD)
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
        Style::default().fg(accent()).add_modifier(Modifier::BOLD)
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
    Style::default().bg(t().log_selection_bg)
}

/// Background for rows inside a visual-line selection (`V` … `y`). Distinct
/// from `log_selection_style` so the range reads as a block, not a single
/// cursor highlight.
pub fn log_visual_style() -> Style {
    Style::default().bg(t().log_visual_bg)
}

/// Reverse badge used for status hints (FOLLOWING / LOCK / VISUAL / flash toasts).
pub fn status_badge(label: &str, bg: Color) -> Span<'static> {
    Span::styled(
        format!(" {label} "),
        Style::default()
            .fg(Color::Black)
            .bg(bg)
            .add_modifier(Modifier::BOLD),
    )
}

/// Dim trailing keybinding hint on the status bar (H6 context help).
pub fn context_help_style() -> Style {
    Style::default().add_modifier(Modifier::DIM)
}

/// Faint search-hit highlight inside the H1 Preview window (distinct from
/// formal [`highlight_style`] / `USER_HIGHLIGHT` chips).
pub fn preview_highlight_style() -> Style {
    Style::default()
        .fg(Color::White)
        .bg(t().preview_highlight_bg)
        .add_modifier(Modifier::DIM)
}

/// Dim style for Preview placeholder / empty state.
pub fn preview_placeholder_style() -> Style {
    Style::default().add_modifier(Modifier::DIM)
}

/// H4 Detail overlay: label for non-chip fields (`time`).
pub fn detail_label_style() -> Style {
    muted().add_modifier(Modifier::DIM)
}

/// H4 Detail overlay: chip-field name tint (matches pill / popup).
pub fn detail_field_label_style(field: ChipField) -> Style {
    Style::default()
        .fg(field_color(field))
        .add_modifier(Modifier::BOLD)
}

/// H3 minimap: empty track (always drawn when `visible` non-empty).
pub fn minimap_track_style() -> Style {
    Style::default()
        .fg(t().border_inactive)
        .add_modifier(Modifier::DIM)
}

/// H3 minimap: approximate viewport band (fainter than marks).
pub fn minimap_viewport_style() -> Style {
    Style::default()
        .fg(t().border_inactive)
        .bg(t().border_inactive)
        .add_modifier(Modifier::DIM)
}

/// H3 minimap: enabled search-hit mark.
pub fn minimap_search_style() -> Style {
    Style::default().fg(accent())
}

/// H3 minimap: severe (E/F/crash) mark — wins over search on overlap.
pub fn minimap_severe_style() -> Style {
    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
}

/// M2 bookmark strip background (subtle wash vs log body).
pub fn bookmark_strip_style() -> Style {
    Style::default().bg(t().bookmark_strip_bg)
}

/// M2 bookmark strip / picker label.
pub fn bookmark_label_style() -> Style {
    Style::default().fg(warning()).add_modifier(Modifier::BOLD)
}

/// M2 stale bookmark (evicted from ring buffer).
pub fn bookmark_stale_style() -> Style {
    Style::default()
        .fg(t().border_inactive)
        .add_modifier(Modifier::DIM | Modifier::CROSSED_OUT)
}

// ── theme.toml parsing (M4) ──────────────────────────────────────────

#[derive(Debug, Deserialize, Default)]
struct ThemeFile {
    accent: Option<String>,
    success: Option<String>,
    warning: Option<String>,
    lock: Option<String>,
    selection_frame: Option<String>,
    log_selection_bg: Option<String>,
    log_visual_bg: Option<String>,
    preview_highlight_bg: Option<String>,
    border_inactive: Option<String>,
    candidate_selection_bg: Option<String>,
    bookmark_strip_bg: Option<String>,
}

/// Parse a theme.toml body into tokens (merged onto builtin defaults).
pub fn parse_theme_toml(text: &str) -> Result<UiTokens, String> {
    let file: ThemeFile = toml::from_str(text).map_err(|e| e.to_string())?;
    let mut t = UiTokens::builtin();
    if let Some(s) = file.accent {
        t.accent = parse_color(&s)?;
    }
    if let Some(s) = file.success {
        t.success = parse_color(&s)?;
    }
    if let Some(s) = file.warning {
        t.warning = parse_color(&s)?;
    }
    if let Some(s) = file.lock {
        t.lock = parse_color(&s)?;
    }
    if let Some(s) = file.selection_frame {
        t.selection_frame = parse_color(&s)?;
    }
    if let Some(s) = file.log_selection_bg {
        t.log_selection_bg = parse_color(&s)?;
    }
    if let Some(s) = file.log_visual_bg {
        t.log_visual_bg = parse_color(&s)?;
    }
    if let Some(s) = file.preview_highlight_bg {
        t.preview_highlight_bg = parse_color(&s)?;
    }
    if let Some(s) = file.border_inactive {
        t.border_inactive = parse_color(&s)?;
    }
    if let Some(s) = file.candidate_selection_bg {
        t.candidate_selection_bg = parse_color(&s)?;
    }
    if let Some(s) = file.bookmark_strip_bg {
        t.bookmark_strip_bg = parse_color(&s)?;
    }
    Ok(t)
}

/// Named ratatui color or `#RRGGBB` / `#RGB`.
pub fn parse_color(s: &str) -> Result<Color, String> {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix('#') {
        return parse_hex(hex);
    }
    match s.to_ascii_lowercase().as_str() {
        "black" => Ok(Color::Black),
        "red" => Ok(Color::Red),
        "green" => Ok(Color::Green),
        "yellow" => Ok(Color::Yellow),
        "blue" => Ok(Color::Blue),
        "magenta" => Ok(Color::Magenta),
        "cyan" => Ok(Color::Cyan),
        "gray" | "grey" => Ok(Color::Gray),
        "darkgray" | "darkgrey" => Ok(Color::DarkGray),
        "lightred" => Ok(Color::LightRed),
        "lightgreen" => Ok(Color::LightGreen),
        "lightyellow" => Ok(Color::LightYellow),
        "lightblue" => Ok(Color::LightBlue),
        "lightmagenta" => Ok(Color::LightMagenta),
        "lightcyan" => Ok(Color::LightCyan),
        "white" => Ok(Color::White),
        other => Err(format!("unknown color '{other}'")),
    }
}

fn parse_hex(hex: &str) -> Result<Color, String> {
    let expand = |c: u8| -> u8 { c * 17 };
    match hex.len() {
        3 => {
            let r = u8::from_str_radix(&hex[0..1], 16).map_err(|e| e.to_string())?;
            let g = u8::from_str_radix(&hex[1..2], 16).map_err(|e| e.to_string())?;
            let b = u8::from_str_radix(&hex[2..3], 16).map_err(|e| e.to_string())?;
            Ok(Color::Rgb(expand(r), expand(g), expand(b)))
        }
        6 => {
            let r = u8::from_str_radix(&hex[0..2], 16).map_err(|e| e.to_string())?;
            let g = u8::from_str_radix(&hex[2..4], 16).map_err(|e| e.to_string())?;
            let b = u8::from_str_radix(&hex[4..6], 16).map_err(|e| e.to_string())?;
            Ok(Color::Rgb(r, g, b))
        }
        _ => Err(format!("invalid hex color '#{hex}'")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_selection_style_is_soft_gray_no_reverse() {
        install(UiTokens::builtin());
        let style = log_selection_style();
        assert_eq!(style.bg, Some(Color::DarkGray));
        assert!(!style.add_modifier.contains(Modifier::REVERSED));
    }

    #[test]
    fn test_candidate_selection_style_soft_gray_with_white_fg() {
        install(UiTokens::builtin());
        let style = candidate_selection_style();
        assert_eq!(style.bg, log_selection_style().bg);
        assert_eq!(style.fg, Some(Color::White));
        assert!(!style.add_modifier.contains(Modifier::REVERSED));
        assert!(!style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn test_log_visual_style_differs_from_selection() {
        install(UiTokens::builtin());
        assert_ne!(log_visual_style().bg, log_selection_style().bg);
    }

    #[test]
    fn test_chip_group_border_style_distinct_from_region_accent() {
        install(UiTokens::builtin());
        assert_ne!(selection_frame(), accent());
        assert_eq!(chip_group_border_style(true).fg, Some(selection_frame()));
    }

    #[test]
    fn test_chip_pill_style_fill() {
        install(UiTokens::builtin());
        let (text, body) = chip_pill_style(ChipField::Tag, "MyTag", false);
        assert_eq!(text, " MyTag ");
        assert_eq!(body.bg, Some(accent()));
    }

    #[test]
    fn test_caret_bar_resets_background() {
        install(UiTokens::builtin());
        let caret = caret_bar();
        assert_eq!(caret.style.fg, Some(accent()));
        // Style::reset() clears inherited pill bg (Color::Reset), not Option::None.
        assert!(
            caret.style.bg.is_none() || caret.style.bg == Some(Color::Reset),
            "caret must not keep a filled bg, got {:?}",
            caret.style.bg
        );
    }

    #[test]
    fn parse_named_and_hex_colors() {
        assert_eq!(parse_color("cyan").unwrap(), Color::Cyan);
        assert_eq!(parse_color("#0ff").unwrap(), Color::Rgb(0, 255, 255));
        assert_eq!(
            parse_color("#112233").unwrap(),
            Color::Rgb(0x11, 0x22, 0x33)
        );
        assert!(parse_color("nope").is_err());
    }

    #[test]
    fn parse_theme_toml_partial_override() {
        let t = parse_theme_toml("accent = \"red\"\n").unwrap();
        assert_eq!(t.accent, Color::Red);
        assert_eq!(t.success, Color::Green);
    }
}
