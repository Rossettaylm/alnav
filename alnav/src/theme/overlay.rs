//! `theme.toml` parsing: palette/token overlays and color literal parsing.

use super::{map_to_tokens_for, UiTokens};
use crate::palette::Palette;
use ratatui::style::Color;
use serde::Deserialize;

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
    highlight: Option<Vec<String>>,
    palette: Option<PaletteFile>,
}

#[derive(Debug, Deserialize, Default)]
struct PaletteFile {
    background: Option<String>,
    foreground: Option<String>,
    black: Option<String>,
    red: Option<String>,
    green: Option<String>,
    yellow: Option<String>,
    blue: Option<String>,
    magenta: Option<String>,
    cyan: Option<String>,
    white: Option<String>,
    bright_black: Option<String>,
    bright_red: Option<String>,
    bright_green: Option<String>,
    bright_yellow: Option<String>,
    bright_blue: Option<String>,
    bright_magenta: Option<String>,
    bright_cyan: Option<String>,
    bright_white: Option<String>,
}

fn apply_palette_file(mut p: Palette, f: PaletteFile) -> Result<Palette, String> {
    let set = |slot: &mut Color, v: Option<String>| -> Result<(), String> {
        if let Some(s) = v {
            *slot = parse_color(&s)?;
        }
        Ok(())
    };
    set(&mut p.background, f.background)?;
    set(&mut p.foreground, f.foreground)?;
    set(&mut p.black, f.black)?;
    set(&mut p.red, f.red)?;
    set(&mut p.green, f.green)?;
    set(&mut p.yellow, f.yellow)?;
    set(&mut p.blue, f.blue)?;
    set(&mut p.magenta, f.magenta)?;
    set(&mut p.cyan, f.cyan)?;
    set(&mut p.white, f.white)?;
    set(&mut p.bright_black, f.bright_black)?;
    set(&mut p.bright_red, f.bright_red)?;
    set(&mut p.bright_green, f.bright_green)?;
    set(&mut p.bright_yellow, f.bright_yellow)?;
    set(&mut p.bright_blue, f.bright_blue)?;
    set(&mut p.bright_magenta, f.bright_magenta)?;
    set(&mut p.bright_cyan, f.bright_cyan)?;
    set(&mut p.bright_white, f.bright_white)?;
    Ok(p)
}

/// Merge `theme.toml` onto the default ANSI palette.
pub fn apply_overlay(base: Palette, text: &str) -> Result<UiTokens, String> {
    apply_overlay_for(base, "default", text)
}

/// Merge `theme.toml` semantic keys and `[palette]` onto a named base palette.
pub fn apply_overlay_for(base: Palette, canonical: &str, text: &str) -> Result<UiTokens, String> {
    let file: ThemeFile = toml::from_str(text).map_err(|e| e.to_string())?;
    if let Some(ref hl) = file.highlight {
        if hl.len() != 8 {
            return Err(format!("highlight must have 8 colors, got {}", hl.len()));
        }
    }
    let pal = if let Some(pf) = file.palette {
        apply_palette_file(base, pf)?
    } else {
        base
    };
    let mut t = map_to_tokens_for(&pal, canonical);
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
    if let Some(hl) = file.highlight {
        let mut colors = [Color::Reset; 8];
        for (i, s) in hl.into_iter().enumerate() {
            colors[i] = parse_color(&s)?;
        }
        t.highlight = colors;
    }
    Ok(t)
}

/// Parse a theme.toml body into tokens (merged onto builtin ANSI defaults).
pub fn parse_theme_toml(text: &str) -> Result<UiTokens, String> {
    apply_overlay(Palette::default_ansi(), text)
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
