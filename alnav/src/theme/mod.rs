//! Single source of truth for alnav's color mapping (see CLAUDE.md
//! "UI 设计指导" for the design rules this module implements). Chrome and
//! log-severity tokens are mapped from a [`crate::palette::Palette`] via
//! [`map_to_tokens`]; style helpers read those tokens, not a separate log palette.
//!
//! UI chrome tokens (accent, selection, preview, …) and `[palette]` / `highlight`
//! overlays may be applied at startup via `theme.toml` (M4).

use crate::input::ChipField;
use crate::palette::{contrast_fg, mix, Palette};
use alnav::parser::Level;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use std::sync::Mutex;

mod glyphs;
mod overlay;
#[cfg(test)]
mod tests;

pub use glyphs::*;
pub use overlay::*;

/// UI chrome + palette-mapped log tokens.
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
    /// Bookmark row wash in LogList (faint lock-family, never overrides cursor/visual).
    pub bookmark_row_bg: Color,
    pub canvas_bg: Color,
    pub canvas_fg: Color,
    pub muted: Color,
    pub error: Color,
    pub pkg: Color,
    pub pid: Color,
    pub tid: Color,
    pub highlight: [Color; 8],
    pub level_v: Color,
    pub level_d: Color,
    pub level_i: Color,
    pub level_w: Color,
    pub level_e: Color,
    pub level_f: Color,
    /// Per-line colors for the six-row Dashboard Unicode wordmark.
    pub logo: [Color; 6],
}

impl UiTokens {
    pub fn builtin() -> Self {
        map_to_tokens(&Palette::default_ansi())
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

pub fn map_to_tokens(p: &Palette) -> UiTokens {
    map_to_tokens_for(p, "default")
}

/// Map `p` through the fixed slot table, then apply the builtin signature
/// (accent + wordmark ramp) for `canonical`. Unknown names behave like `default`.
pub fn map_to_tokens_for(p: &Palette, canonical: &str) -> UiTokens {
    let canonical = crate::palette::resolve_theme_name(canonical).unwrap_or("default");
    let accent = signature_accent(p, canonical);
    let washes_from_mix = !matches!(p.background, Color::Reset);
    let mix_or = |tint: Color, t: u8, fallback: Color| -> Color {
        if washes_from_mix {
            mix(p.background, tint, t).unwrap_or(fallback)
        } else {
            fallback
        }
    };
    UiTokens {
        accent,
        success: p.green,
        warning: p.yellow,
        lock: p.magenta,
        selection_frame: p.magenta,
        log_selection_bg: mix_or(accent, 22, Color::DarkGray),
        log_visual_bg: mix_or(p.blue, 26, Color::Rgb(30, 60, 70)),
        preview_highlight_bg: mix_or(accent, 28, Color::DarkGray),
        border_inactive: p.bright_black,
        candidate_selected_bg: mix_or(p.foreground, 16, Color::DarkGray),
        candidate_selected_fg: if washes_from_mix {
            p.bright_white
        } else {
            Color::White
        },
        candidate_unselected_bg: if washes_from_mix {
            p.background
        } else {
            Color::Reset
        },
        candidate_unselected_fg: if washes_from_mix {
            p.white
        } else {
            Color::Gray
        },
        candidate_match_fg: accent,
        candidate_prefix: "▌ ".to_string(),
        bookmark_strip_bg: mix_or(p.magenta, 6, Color::DarkGray),
        bookmark_row_bg: mix_or(p.magenta, 8, Color::Rgb(36, 24, 40)),
        canvas_bg: p.background,
        canvas_fg: p.foreground,
        muted: p.bright_black,
        error: p.red,
        pkg: p.bright_yellow,
        pid: p.magenta,
        tid: p.bright_magenta,
        highlight: [
            p.yellow,
            p.bright_yellow,
            p.red,
            p.magenta,
            p.blue,
            p.cyan,
            p.green,
            p.bright_green,
        ],
        level_v: p.bright_black,
        level_d: p.blue,
        level_i: p.green,
        level_w: p.yellow,
        level_e: p.red,
        level_f: p.bright_red,
        logo: logo_ramp(p, canonical, accent),
    }
}

fn signature_accent(p: &Palette, canonical: &str) -> Color {
    match canonical {
        "onedark" | "tokyo-night" | "kanagawa" => p.blue,
        "dracula" | "catppuccin-mocha" => p.magenta,
        "everforest" => p.green,
        "gruvbox-dark" => p.yellow,
        _ => p.cyan,
    }
}

fn logo_ramp(p: &Palette, canonical: &str, accent: Color) -> [Color; 6] {
    let (a, b, c) = match canonical {
        "onedark" => (p.blue, p.cyan, p.magenta),
        "dracula" => (p.cyan, p.magenta, p.blue),
        "everforest" => (p.green, p.yellow, p.cyan),
        "tokyo-night" => (p.blue, p.magenta, p.cyan),
        "catppuccin-mocha" => (p.blue, p.magenta, p.red),
        "gruvbox-dark" => (p.yellow, p.red, p.green),
        "nord" => (p.blue, p.cyan, p.bright_cyan),
        "kanagawa" => (p.blue, p.magenta, p.cyan),
        _ => return [accent; 6],
    };
    [
        a,
        mix(a, b, 40).unwrap_or(a),
        mix(a, b, 80).unwrap_or(b),
        b,
        mix(b, c, 50).unwrap_or(c),
        c,
    ]
}

pub fn canvas_bg() -> Color {
    t().canvas_bg
}

pub fn canvas_style() -> Style {
    let tk = t();
    let mut style = Style::default();
    if !matches!(tk.canvas_bg, Color::Reset) {
        style = style.bg(tk.canvas_bg);
    }
    if !matches!(tk.canvas_fg, Color::Reset) {
        style = style.fg(tk.canvas_fg);
    }
    style
}

/// Timestamp/pid/tid/separator tint from the active palette's muted slot.
pub fn muted() -> Style {
    Style::default().fg(t().muted)
}

/// Right-aligned key chord in the command palette candidate list.
pub fn palette_keyhint_style() -> Style {
    muted().add_modifier(Modifier::DIM)
}

/// Tag/message foreground for severe log rows (E/F or crash signature).
/// `emphatic` is Fatal and crash lines (bold); Error-level stays red without bold.
pub fn severe_entry_style(emphatic: bool) -> Style {
    let mut style = Style::default().fg(t().error);
    if emphatic {
        style = style.add_modifier(Modifier::BOLD);
    }
    style
}

/// Colored level badge (e.g. `" E "` on a red background).
pub fn level_badge_style(level: Level) -> Style {
    let bg = match level {
        Level::V => t().level_v,
        Level::D => t().level_d,
        Level::I => t().level_i,
        Level::W => t().level_w,
        Level::E => t().level_e,
        Level::F => t().level_f,
    };
    let mut style = Style::default().fg(contrast_fg(bg)).bg(bg);
    if matches!(level, Level::F) {
        style = style.add_modifier(Modifier::BOLD);
    }
    style
}

/// Foreground-only variant of [`level_badge_style`]'s color, for the summary
/// panel's per-level bar chart (bar glyphs, not badge text-on-bg).
pub fn level_bar_style(level: Level) -> Style {
    let bg = level_badge_style(level).bg.unwrap_or(Color::White);
    Style::default().fg(bg)
}

/// Bar-chart color for the summary panel's Top-tags section (single accent).
pub fn accent_bar_style() -> Style {
    Style::default().fg(accent())
}

/// One of the 8 palette highlight colors, cycled by index.
/// TUI search chips assign a progressive index per pattern.
pub fn highlight_style(idx: usize) -> Style {
    let tk = t();
    let bg = tk.highlight[idx % tk.highlight.len()];
    Style::default()
        .fg(contrast_fg(bg))
        .bg(bg)
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

/// Chip field -> accent color, shared by the input box, popup, and (once
/// committed) the chip strip so a field always reads the same color
/// everywhere it appears.
pub fn field_color(field: ChipField) -> Color {
    match field {
        ChipField::Tag => accent(),
        ChipField::Msg => success(),
        ChipField::Pkg => t().pkg,
        ChipField::Pid => t().pid,
        ChipField::Tid => t().tid,
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
/// Named themes (painted canvas) use white + DIM; default stays Gray without DIM.
pub fn candidate_unselected_style() -> Style {
    let tk = t();
    let mut style = Style::default()
        .fg(tk.candidate_unselected_fg)
        .bg(tk.candidate_unselected_bg);
    if !matches!(tk.canvas_bg, Color::Reset) {
        style = style.add_modifier(Modifier::DIM);
    }
    style
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

/// Compact / Minimal Dashboard wordmark (`"alnav"`): signature accent.
pub fn dashboard_header_style() -> Style {
    Style::default().fg(accent()).add_modifier(Modifier::BOLD)
}

/// One row of the six-line Unicode wordmark.
pub fn dashboard_logo_line_style(row: usize) -> Style {
    Style::default()
        .fg(t().logo[row % 6])
        .add_modifier(Modifier::BOLD)
}

/// Dashboard product subtitle, empty-state copy, and footer.
/// Named (painted) themes use canvas foreground so copy tracks the scheme;
/// `default` keeps muted DarkGray.
pub fn dashboard_muted_style() -> Style {
    let tk = t();
    let fg = if matches!(tk.canvas_fg, Color::Reset) {
        tk.muted
    } else {
        tk.canvas_fg
    };
    Style::default().fg(fg).add_modifier(Modifier::DIM)
}

/// Borderless Dashboard section heading.
pub fn dashboard_section_style() -> Style {
    Style::default()
        .fg(accent())
        .add_modifier(Modifier::BOLD | Modifier::DIM)
}

/// Base style for a Dashboard action/recent row.
pub fn dashboard_item_style(selected: bool) -> Style {
    if selected {
        candidate_selected_style()
    } else {
        candidate_unselected_style()
    }
}

/// Dim secondary copy while preserving the selected row's soft background.
pub fn dashboard_description_style(selected: bool) -> Style {
    let bg = dashboard_item_style(selected).bg;
    dashboard_muted_style().bg(bg.unwrap_or(Color::Reset))
}

/// Right-aligned Dashboard shortcut badge.
pub fn dashboard_hotkey_style(selected: bool) -> Style {
    let bg = dashboard_item_style(selected).bg;
    Style::default()
        .fg(accent())
        .bg(bg.unwrap_or(Color::Reset))
        .add_modifier(Modifier::BOLD)
}

/// Dashboard-local transient failure text.
pub fn dashboard_flash_style() -> Style {
    Style::default().fg(warning()).add_modifier(Modifier::DIM)
}

/// Soft accent+DIM style for picker mode prefixes (no fill — distinct from chip pills).
pub fn picker_mode_style() -> Style {
    Style::default().fg(accent()).add_modifier(Modifier::DIM)
}

/// Soft prompt icon + two trailing spaces (gap before draft text).
pub fn picker_prompt_prefix(icon: &'static str) -> Span<'static> {
    Span::styled(format!("{icon}  "), picker_mode_style())
}

/// Mode prefix icon (nerdfont): Manage search, New plus-square, Edit pencil.
/// Bookmark panels pass [`GLYPH_BOOKMARK`] via [`picker_prompt_prefix`] instead.
pub fn picker_mode_prefix(mode: &crate::picker::PickerMode) -> Span<'static> {
    let icon = match mode {
        crate::picker::PickerMode::Manage => GLYPH_MODE_MANAGE,
        crate::picker::PickerMode::New => GLYPH_MODE_NEW,
        crate::picker::PickerMode::Edit { .. } => GLYPH_MODE_EDIT,
    };
    picker_prompt_prefix(icon)
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
            .fg(contrast_fg(field_color(other)))
            .bg(field_color(other))
            .add_modifier(Modifier::BOLD),
    };
    vec![Span::styled(body_text, body_style)]
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
            .fg(contrast_fg(field_color(other)))
            .bg(field_color(other))
            .add_modifier(Modifier::BOLD),
    };
    vec![Span::styled(body_text, body_style)]
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
    numbered_title_with_loading(number, label, active, None)
}

/// Like [`numbered_title`], with an optional dim loading suffix (LogList L1 banner).
pub fn numbered_title_with_loading(
    number: u8,
    label: &str,
    active: bool,
    loading: Option<&str>,
) -> Line<'static> {
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
    let mut spans = vec![
        Span::styled(format!(" {glyph} {number} "), badge_style),
        Span::styled(format!(" {label} "), label_style),
    ];
    if let Some(msg) = loading {
        spans.push(Span::styled(format!(" {msg} "), log_loading_style(active)));
    }
    Line::from(spans)
}

/// Style for LogList index/filter/highlight progress text in the block title.
pub fn log_loading_style(active: bool) -> Style {
    if active {
        Style::default()
            .fg(accent())
            .add_modifier(Modifier::DIM | Modifier::ITALIC)
    } else {
        Style::default().add_modifier(Modifier::DIM | Modifier::ITALIC)
    }
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

fn status_pill_style(fg: Color) -> Style {
    Style::default()
        .fg(contrast_fg(fg))
        .bg(fg)
        .add_modifier(Modifier::BOLD)
}

/// Filled status pill (` {glyph} `). On-pill fg from [`contrast_fg`].
pub fn status_pill(glyph: &str, fg: Color) -> Span<'static> {
    Span::styled(format!(" {glyph} "), status_pill_style(fg))
}

/// Filled status pill with a short value (` {glyph} {value} `).
pub fn status_pill_value(glyph: &str, value: &str, fg: Color) -> Span<'static> {
    Span::styled(format!(" {glyph} {value} "), status_pill_style(fg))
}

/// Follow off-state: same slot shape as [`status_pill`], DIM, no fill.
pub fn status_icon_dim(glyph: &str) -> Span<'static> {
    Span::styled(
        format!(" {glyph} "),
        Style::default().add_modifier(Modifier::DIM),
    )
}

fn flash_fill(text: &str) -> Color {
    if text.contains("FAILED") {
        warning()
    } else {
        success()
    }
}

/// Filled flash toast. Warning fill when the copy contains `FAILED`.
pub fn status_flash_pill(text: &str) -> Span<'static> {
    status_flash_pill_fit(text, usize::MAX)
}

/// Truncate flash copy so the pill (` {text} `) fits `max_chars`.
pub fn status_flash_pill_fit(text: &str, max_chars: usize) -> Span<'static> {
    let fg = flash_fill(text);
    if max_chars == 0 {
        return Span::raw("");
    }
    let full = format!(" {text} ");
    let shown = if full.chars().count() <= max_chars {
        full
    } else {
        let inner = max_chars.saturating_sub(2);
        let body: String = text.chars().take(inner).collect();
        if body.is_empty() {
            " ".repeat(max_chars.min(2))
        } else {
            format!(" {body} ")
        }
    };
    Span::styled(shown, status_pill_style(fg))
}

/// Dim trailing keybinding hint on the status bar (H6 context help).
pub fn context_help_style() -> Style {
    Style::default().add_modifier(Modifier::DIM)
}

/// Substring hit inside the Help panel (non-current).
pub fn help_search_hit_style() -> Style {
    highlight_style(0)
}

/// Current Help search hit (same ramp slot, underlined).
pub fn help_search_current_style() -> Style {
    highlight_style_active(0)
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
    Style::default().fg(t().error).add_modifier(Modifier::BOLD)
}

/// M2 bookmark strip background (subtle wash vs log body).
pub fn bookmark_strip_style() -> Style {
    Style::default().bg(t().bookmark_strip_bg)
}

/// M2 bookmark strip / picker label.
pub fn bookmark_label_style() -> Style {
    Style::default().fg(warning()).add_modifier(Modifier::BOLD)
}

/// LogList row wash for bookmarked rows (faint lock). Priority:
/// `visual > cursor-selection > bookmark-wash` (see `ui::render_log_list`).
pub fn bookmark_row_style() -> Style {
    Style::default().bg(t().bookmark_row_bg)
}

/// Lineno-gutter pin glyph on bookmarked LogList rows.
pub fn bookmark_mark_style() -> Style {
    Style::default().fg(lock()).add_modifier(Modifier::DIM)
}

/// Foreground color for the minimap Bookmark mark (F5). Lock family, same
/// as the gutter pin, not the row wash.
pub fn bookmark_minimap_color() -> Color {
    lock()
}

/// M2 stale bookmark (evicted from ring buffer).
pub fn bookmark_stale_style() -> Style {
    Style::default()
        .fg(t().border_inactive)
        .add_modifier(Modifier::DIM | Modifier::CROSSED_OUT)
}

/// Compare-panel Δt prefix (`+1.2s` / `—`).
pub fn compare_delta_style() -> Style {
    muted().add_modifier(Modifier::DIM)
}

/// Unified Manage list: kind prefix / row tint by category.
pub fn unified_kind_style(kind: crate::picker::UnifiedKind) -> Style {
    use crate::picker::UnifiedKind;
    match kind {
        UnifiedKind::Filter => Style::default().fg(accent()),
        UnifiedKind::Highlight => Style::default().fg(t().highlight[0]),
        UnifiedKind::Exclude => Style::default().fg(warning()),
    }
}

/// Candidate-list prefix when the row is Tab multi-selected (checked).
pub fn candidate_checked_prefix_style() -> Style {
    Style::default().fg(lock()).add_modifier(Modifier::BOLD)
}
