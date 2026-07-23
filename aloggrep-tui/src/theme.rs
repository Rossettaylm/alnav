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

// ---------------------------------------------------------------------------
// Nerdfont semantic glyphs (hard dependency — no runtime fallback).
// All UI iconography must reference these constants; `ui.rs` MUST NOT inline
// glyph literals. See prd.md R4 / design.md D1 for the rationale and table.
// ---------------------------------------------------------------------------

pub const GLYPH_MODE_MANAGE: &str = "\u{f0b7}"; //
pub const GLYPH_MODE_NEW: &str = "\u{f0fe}"; //
pub const GLYPH_MODE_EDIT: &str = "\u{f044}"; //
pub const GLYPH_CARET_SEL: &str = "\u{f0da}"; //
pub const GLYPH_TITLE_PICKER: &str = "\u{f002}"; //
pub const GLYPH_TITLE_LOG: &str = "\u{f0c5}"; //
pub const GLYPH_TITLE_FILTER: &str = "\u{f0b0}"; //
pub const GLYPH_TITLE_EXCLUDE: &str = "\u{f056}"; //
pub const GLYPH_TITLE_HIGHLIGHT: &str = "\u{f0e0}"; //
pub const GLYPH_GROUP_ON: &str = "\u{f192}"; //
pub const GLYPH_GROUP_OFF: &str = "\u{f10c}"; //
pub const GLYPH_BOOKMARK: &str = "\u{f02e}"; //
pub const GLYPH_ACTION_JUMP: &str = "\u{f061}"; //  nf-fa-arrow_right
pub const GLYPH_ACTION_TOGGLE_ON: &str = "\u{f205}"; //  nf-fa-toggle_on
pub const GLYPH_ACTION_TOGGLE_OFF: &str = "\u{f204}"; //  nf-fa-toggle_off
pub const GLYPH_LOCK: &str = "\u{f023}"; //
pub const GLYPH_FOLLOWING: &str = "\u{f062}"; //
pub const GLYPH_VISUAL: &str = "\u{f245}"; //
pub const GLYPH_SEARCH: &str = "\u{f002}"; //
pub const GLYPH_CRASH: &str = "\u{f071}"; //
pub const GLYPH_SEP: &str = "\u{e0bf}"; //
pub const GLYPH_FIELD_TAG: &str = "\u{f02b}"; //
pub const GLYPH_FIELD_MSG: &str = "\u{f075}"; //
pub const GLYPH_FIELD_PKG: &str = "\u{f187}"; //
pub const GLYPH_FIELD_PID: &str = "\u{f292}"; //
pub const GLYPH_FIELD_TID: &str = "\u{f2bd}"; //
pub const GLYPH_FIELD_LEVEL: &str = "\u{f0d0}"; //
pub const GLYPH_HR: &str = "\u{2500}"; // ─

/// Map a chip field to its nerdfont icon glyph.
pub fn field_icon(field: ChipField) -> &'static str {
    match field {
        ChipField::Tag => GLYPH_FIELD_TAG,
        ChipField::Msg => GLYPH_FIELD_MSG,
        ChipField::Pkg => GLYPH_FIELD_PKG,
        ChipField::Pid => GLYPH_FIELD_PID,
        ChipField::Tid => GLYPH_FIELD_TID,
        ChipField::Level => GLYPH_FIELD_LEVEL,
    }
}

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
    /// Selected candidate row background (picker / field popup).
    pub candidate_selected_bg: Color,
    /// Selected candidate row text color.
    pub candidate_selected_fg: Color,
    /// Unselected candidate row background (`Reset` = inherit terminal).
    pub candidate_unselected_bg: Color,
    /// Unselected candidate row text color.
    pub candidate_unselected_fg: Color,
    /// Substring match characters inside candidate labels.
    pub candidate_match_fg: Color,
    /// Prefix drawn before the selected candidate row (e.g. `"▌ "`).
    pub candidate_prefix: String,
    pub bookmark_strip_bg: Color,
    /// Bookmark row background in LogList (faint yellow, distinct from selection).
    pub bookmark_row_bg: Color,
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
            candidate_selected_bg: Color::DarkGray,
            candidate_selected_fg: Color::White,
            candidate_unselected_bg: Color::Reset,
            candidate_unselected_fg: Color::Gray,
            candidate_match_fg: Color::Cyan,
            candidate_prefix: "▌ ".to_string(),
            bookmark_strip_bg: Color::DarkGray,
            bookmark_row_bg: Color::Rgb(54, 46, 0),
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
pub fn highlight_match_status_style() -> Style {
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

/// Selected candidate row (picker / field popup).
pub fn candidate_selected_style() -> Style {
    Style::default()
        .fg(t().candidate_selected_fg)
        .bg(t().candidate_selected_bg)
}

/// Unselected candidate row base style.
pub fn candidate_unselected_style() -> Style {
    Style::default()
        .fg(t().candidate_unselected_fg)
        .bg(t().candidate_unselected_bg)
}

/// Match-character foreground for candidate substring hits.
pub fn candidate_match_style(selected: bool) -> Style {
    let bg = if selected {
        t().candidate_selected_bg
    } else {
        t().candidate_unselected_bg
    };
    Style::default().fg(t().candidate_match_fg).bg(bg)
}

/// Prefix string for the selected candidate row (nerdfont caret-right glyph).
pub fn candidate_prefix() -> String {
    format!("{} ", GLYPH_CARET_SEL)
}

/// Backward-compatible alias for selected candidate style.
pub fn candidate_selection_style() -> Style {
    candidate_selected_style()
}

/// Soft accent+DIM style for picker mode prefixes (no fill — distinct from chip pills).
pub fn picker_mode_style() -> Style {
    Style::default().fg(accent()).add_modifier(Modifier::DIM)
}

/// Mode prefix icon (nerdfont): Manage ``, New ``, Edit ``.
pub fn picker_mode_prefix(mode: &crate::picker::PickerMode) -> Span<'static> {
    let icon = match mode {
        crate::picker::PickerMode::Manage => GLYPH_MODE_MANAGE,
        crate::picker::PickerMode::New => GLYPH_MODE_NEW,
        crate::picker::PickerMode::Edit { .. } => GLYPH_MODE_EDIT,
    };
    Span::styled(format!("{icon} "), picker_mode_style())
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

/// Build a filter pill as a single space-filled span (field-colored bg) with
/// the field icon prefixing the value. No powerline ends (Q3: weakened chrome).
/// `disabled` collapses to a single dim span (same shape, dim style).
pub fn chip_pill_spans(field: ChipField, value: &str, disabled: bool) -> Vec<Span<'static>> {
    let icon = field_icon(field);
    if disabled {
        let text = format!(" {icon} {value} ");
        return vec![Span::styled(text, disabled_chip_style())];
    }
    let body_text = format!(" {icon} {value} ");
    let body_style = match field {
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
    vec![Span::styled(body_text, body_style)]
}

/// Backward-compatible single-span pill (tests / callers that don't need
/// powerline ends). Returns body text + style only.
pub fn chip_pill_style(field: ChipField, value: &str, disabled: bool) -> (String, Style) {
    if disabled {
        return (format!(" {value} "), disabled_chip_style());
    }
    let icon = field_icon(field);
    let text = format!(" {icon} {value} ");
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

/// Exclude pill (H9): space-filled pill with a `!` prefix before the field icon.
pub fn exclude_pill_spans(field: ChipField, value: &str, disabled: bool) -> Vec<Span<'static>> {
    let icon = field_icon(field);
    if disabled {
        let text = format!(" !{icon} {value} ");
        return vec![Span::styled(text, disabled_chip_style())];
    }
    let body_text = format!(" !{icon} {value} ");
    let body_style = match field {
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
    vec![Span::styled(body_text, body_style)]
}

/// Backward-compatible single-span exclude pill.
pub fn exclude_pill_style(field: ChipField, value: &str, disabled: bool) -> (String, Style) {
    let (inner, style) = chip_pill_style(field, value, disabled);
    (format!("!{inner}"), style)
}

/// Search/highlight pill as a single space-filled span.
/// `active_global` underlines the globally active (n/N) search chip.
pub fn highlight_pill_spans(
    pattern: &str,
    color_idx: usize,
    disabled: bool,
    active_global: bool,
) -> Vec<Span<'static>> {
    if disabled {
        let text = format!(" {pattern} ");
        return vec![Span::styled(text, disabled_chip_style())];
    }
    let style = if active_global {
        highlight_style_active(color_idx)
    } else {
        highlight_style(color_idx)
    };
    vec![Span::styled(format!(" {pattern} "), style)]
}

/// Backward-compatible single-span highlight pill.
pub fn highlight_pill_style(
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

/// Thin editing caret at end-of-line (no character under the caret).
///
/// Uses [`Style::reset`] so a preceding filled pill cannot leave its `bg` on
/// the caret cell (`Cell::set_style` only applies `Some(_)` fields).
pub fn caret_bar() -> Span<'static> {
    Span::styled(
        "▏",
        Style::reset().fg(accent()).add_modifier(Modifier::BOLD),
    )
}

/// Block caret painted *on* the character under the cursor (mid-string).
/// Does not insert an extra glyph, so moving the caret never shifts text.
pub fn caret_block_style() -> Style {
    Style::reset()
        .fg(Color::Black)
        .bg(accent())
        .add_modifier(Modifier::BOLD)
}

/// Border color for a bordered region: dimmed accent when it currently has
/// keyboard focus (reduced from full-saturation accent per Q3 border-weakening),
/// dim gray otherwise.
pub fn border_style(active: bool) -> Style {
    if active {
        Style::default().fg(accent()).add_modifier(Modifier::DIM)
    } else {
        Style::default()
            .fg(t().border_inactive)
            .add_modifier(Modifier::DIM)
    }
}

/// Glyph for a numbered region, chosen by its Tab-cycle digit.
fn numbered_glyph(number: u8) -> &'static str {
    match number {
        1 => GLYPH_TITLE_FILTER,
        2 => GLYPH_TITLE_EXCLUDE,
        3 => GLYPH_TITLE_HIGHLIGHT,
        4 => GLYPH_TITLE_LOG,
        5 => GLYPH_TITLE_PICKER,
        _ => GLYPH_TITLE_PICKER,
    }
}

/// Border title for a numbered, Tab-cyclable region (Filter/Exclude/Highlight/Log/Input):
/// a nerdfont glyph + digit badge + label, styled by whether the region is
/// currently focused. No reverse-color block (Q3: weakened borders).
pub fn numbered_title(number: u8, label: &str, active: bool) -> Line<'static> {
    let glyph = numbered_glyph(number);
    let badge_style = if active {
        Style::default().fg(accent()).add_modifier(Modifier::BOLD)
    } else {
        Style::default().add_modifier(Modifier::DIM)
    };
    let label_style = if active {
        Style::default().fg(accent()).add_modifier(Modifier::BOLD)
    } else {
        Style::default().add_modifier(Modifier::DIM)
    };
    Line::from(vec![
        Span::styled(format!(" {glyph} {number} "), badge_style),
        Span::styled(format!(" {label} "), label_style),
    ])
}

/// Border title for a region that isn't part of the numbered Tab cycle
/// (the search box, the field popup). Prepends a nerdfont glyph.
pub fn plain_title(glyph: &str, label: &str, active: bool) -> Line<'static> {
    let label_style = if active {
        Style::default().fg(accent()).add_modifier(Modifier::BOLD)
    } else {
        Style::default().add_modifier(Modifier::DIM)
    };
    Line::from(Span::styled(format!(" {glyph} {label} "), label_style))
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

/// Status badge: nerdfont glyph + label in a semantic foreground color.
/// No reverse-color block (Q3: weakened chrome). Pass `""` for `glyph` when
/// no icon is appropriate (pending-state shorthand, flash toasts).
pub fn status_badge(glyph: &str, label: &str, fg: Color) -> Span<'static> {
    let text = if glyph.is_empty() {
        format!(" {label} ")
    } else {
        format!(" {glyph} {label} ")
    };
    Span::styled(text, Style::default().fg(fg).add_modifier(Modifier::BOLD))
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
pub fn minimap_highlight_style() -> Style {
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
/// LogList row background for bookmarked rows (faint yellow). Priority:
/// `visual > bookmark-bg > cursor-selection` (see `ui::render_log_list`).
pub fn bookmark_row_style() -> Style {
    Style::default().bg(t().bookmark_row_bg)
}

/// Foreground color for the minimap Bookmark mark (F5). Same color family as
/// the bookmark row bg so the rail mark reads as related to the row wash.
pub fn bookmark_minimap_color() -> Color {
    t().bookmark_row_bg
}

/// M2 stale bookmark (evicted from ring buffer).
pub fn bookmark_stale_style() -> Style {
    Style::default()
        .fg(t().border_inactive)
        .add_modifier(Modifier::DIM | Modifier::CROSSED_OUT)
}

/// Unified Manage list: kind prefix / row tint by category.
pub fn unified_kind_style(kind: crate::picker::UnifiedKind) -> Style {
    use crate::picker::UnifiedKind;
    match kind {
        UnifiedKind::Filter => Style::default().fg(accent()),
        UnifiedKind::Highlight => {
            let ((r, g, b), _) = logcolor::USER_HIGHLIGHT[0];
            Style::default().fg(Color::Rgb(r, g, b))
        }
        UnifiedKind::Exclude => Style::default().fg(warning()),
    }
}

/// Candidate-list prefix when the row is Tab multi-selected (checked).
pub fn candidate_checked_prefix_style() -> Style {
    Style::default().fg(lock()).add_modifier(Modifier::BOLD)
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
    candidate_selected_bg: Option<String>,
    candidate_selected_fg: Option<String>,
    candidate_unselected_bg: Option<String>,
    candidate_unselected_fg: Option<String>,
    candidate_match_fg: Option<String>,
    candidate_prefix: Option<String>,
    /// Deprecated alias for [`Self::candidate_selected_bg`].
    candidate_selection_bg: Option<String>,
    bookmark_strip_bg: Option<String>,
    bookmark_row_bg: Option<String>,
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
    if let Some(s) = file.candidate_selected_bg {
        t.candidate_selected_bg = parse_color(&s)?;
    } else if let Some(s) = file.candidate_selection_bg {
        t.candidate_selected_bg = parse_color(&s)?;
    }
    if let Some(s) = file.candidate_selected_fg {
        t.candidate_selected_fg = parse_color(&s)?;
    }
    if let Some(s) = file.candidate_unselected_bg {
        t.candidate_unselected_bg = parse_color(&s)?;
    }
    if let Some(s) = file.candidate_unselected_fg {
        t.candidate_unselected_fg = parse_color(&s)?;
    }
    if let Some(s) = file.candidate_match_fg {
        t.candidate_match_fg = parse_color(&s)?;
    }
    if let Some(s) = file.candidate_prefix {
        t.candidate_prefix = s;
    }
    if let Some(s) = file.bookmark_strip_bg {
        t.bookmark_strip_bg = parse_color(&s)?;
    }
    if let Some(s) = file.bookmark_row_bg {
        t.bookmark_row_bg = parse_color(&s)?;
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
        "reset" => Ok(Color::Reset),
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
        assert_eq!(style.bg, Some(Color::DarkGray));
        assert_eq!(style.fg, Some(Color::White));
        assert!(!style.add_modifier.contains(Modifier::REVERSED));
        assert!(!style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn test_candidate_match_and_prefix_tokens() {
        install(UiTokens::builtin());
        assert_eq!(candidate_match_style(false).fg, Some(Color::Cyan));
        assert_eq!(candidate_prefix(), format!("{} ", GLYPH_CARET_SEL));
        use crate::picker::PickerMode;
        assert_eq!(
            picker_mode_prefix(&PickerMode::Manage).content,
            format!("{} ", GLYPH_MODE_MANAGE)
        );
        assert_eq!(
            picker_mode_prefix(&PickerMode::New).content,
            format!("{} ", GLYPH_MODE_NEW)
        );
        assert_eq!(
            picker_mode_prefix(&PickerMode::Edit { index: 0 }).content,
            format!("{} ", GLYPH_MODE_EDIT)
        );
        let soft = picker_mode_style();
        assert_eq!(soft.fg, Some(Color::Cyan));
        assert!(soft.add_modifier.contains(Modifier::DIM));
        assert_eq!(soft.bg, None);
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
        assert_eq!(text, format!(" {} MyTag ", GLYPH_FIELD_TAG));
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
