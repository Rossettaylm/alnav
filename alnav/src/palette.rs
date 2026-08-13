use ratatui::style::Color;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    pub background: Color,
    pub foreground: Color,
    pub black: Color,
    pub red: Color,
    pub green: Color,
    pub yellow: Color,
    pub blue: Color,
    pub magenta: Color,
    pub cyan: Color,
    pub white: Color,
    pub bright_black: Color,
    pub bright_red: Color,
    pub bright_green: Color,
    pub bright_yellow: Color,
    pub bright_blue: Color,
    pub bright_magenta: Color,
    pub bright_cyan: Color,
    pub bright_white: Color,
}

impl Palette {
    pub fn default_ansi() -> Self {
        Self {
            background: Color::Reset,
            foreground: Color::Reset,
            black: Color::Black,
            red: Color::Red,
            green: Color::Green,
            yellow: Color::Yellow,
            blue: Color::Blue,
            magenta: Color::Magenta,
            cyan: Color::Cyan,
            white: Color::White,
            bright_black: Color::DarkGray,
            bright_red: Color::LightRed,
            bright_green: Color::LightGreen,
            bright_yellow: Color::LightYellow,
            bright_blue: Color::LightBlue,
            bright_magenta: Color::LightMagenta,
            bright_cyan: Color::LightCyan,
            bright_white: Color::White,
        }
    }
}

pub fn fold_theme_name(raw: &str) -> String {
    raw.trim().to_ascii_lowercase().replace('_', "-")
}

pub fn resolve_theme_name(raw: &str) -> Option<&'static str> {
    match fold_theme_name(raw).as_str() {
        "default" | "builtin" => Some("default"),
        "onedark" | "one-dark" => Some("onedark"),
        "dracula" => Some("dracula"),
        "everforest" | "ever-forest" => Some("everforest"),
        "tokyo-night" | "tokyonight" => Some("tokyo-night"),
        "catppuccin-mocha" | "catppuccin" | "mocha" => Some("catppuccin-mocha"),
        "gruvbox-dark" | "gruvbox" => Some("gruvbox-dark"),
        "nord" => Some("nord"),
        "kanagawa" | "kanagawa-wave" => Some("kanagawa"),
        _ => None,
    }
}

/// Linear mix. `t` is 0..=100. Returns `None` unless both ends are `Color::Rgb`.
pub fn mix(bg: Color, tint: Color, t: u8) -> Option<Color> {
    let t = u16::from(t.min(100));
    match (bg, tint) {
        (Color::Rgb(br, bg_, bb), Color::Rgb(tr, tg, tb)) => {
            let ch = |a: u8, b: u8| -> u8 {
                let a = u16::from(a);
                let b = u16::from(b);
                ((a * (100 - t) + b * t) / 100) as u8
            };
            Some(Color::Rgb(ch(br, tr), ch(bg_, tg), ch(bb, tb)))
        }
        _ => None,
    }
}

pub fn contrast_fg(bg: Color) -> Color {
    match bg {
        Color::Rgb(r, g, b) => {
            let y = 0.299 * f32::from(r) + 0.587 * f32::from(g) + 0.114 * f32::from(b);
            if y >= 140.0 {
                Color::Black
            } else {
                Color::White
            }
        }
        Color::Yellow
        | Color::LightYellow
        | Color::LightGreen
        | Color::LightCyan
        | Color::LightBlue
        | Color::LightMagenta
        | Color::LightRed
        | Color::White
        | Color::Gray => Color::Black,
        Color::Green | Color::Blue | Color::Cyan => Color::Black,
        _ => Color::White,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fold_and_resolve_aliases() {
        assert_eq!(resolve_theme_name("TokyoNight"), Some("tokyo-night"));
        assert_eq!(resolve_theme_name("tokyo_night"), Some("tokyo-night"));
        assert_eq!(resolve_theme_name(" mocha "), Some("catppuccin-mocha"));
        assert_eq!(resolve_theme_name("one_dark"), Some("onedark"));
        assert_eq!(resolve_theme_name("kanagawa-wave"), Some("kanagawa"));
        assert_eq!(resolve_theme_name("not-a-theme"), None);
        assert_eq!(resolve_theme_name("latte"), None);
    }

    #[test]
    fn mix_rgb_and_reject_reset() {
        let bg = Color::Rgb(0, 0, 0);
        let tint = Color::Rgb(100, 0, 0);
        assert_eq!(mix(bg, tint, 50), Some(Color::Rgb(50, 0, 0)));
        assert_eq!(mix(Color::Reset, tint, 15), None);
    }

    #[test]
    fn contrast_fg_luminance_threshold() {
        assert_eq!(contrast_fg(Color::Rgb(255, 255, 0)), Color::Black);
        assert_eq!(contrast_fg(Color::Rgb(0, 0, 0)), Color::White);
        assert_eq!(contrast_fg(Color::Yellow), Color::Black);
        assert_eq!(contrast_fg(Color::Red), Color::White);
        assert_eq!(contrast_fg(Color::DarkGray), Color::White);
    }
}
