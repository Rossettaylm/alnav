use ratatui::style::Color;

use crate::palette::{resolve_theme_name, Palette};

const fn rgb(hex: u32) -> Color {
    Color::Rgb(
        ((hex >> 16) & 0xff) as u8,
        ((hex >> 8) & 0xff) as u8,
        (hex & 0xff) as u8,
    )
}

fn hex_palette(
    background: u32,
    foreground: u32,
    black: u32,
    red: u32,
    green: u32,
    yellow: u32,
    blue: u32,
    magenta: u32,
    cyan: u32,
    white: u32,
    bright_black: u32,
    bright_red: u32,
    bright_green: u32,
    bright_yellow: u32,
    bright_blue: u32,
    bright_magenta: u32,
    bright_cyan: u32,
    bright_white: u32,
) -> Palette {
    Palette {
        background: rgb(background),
        foreground: rgb(foreground),
        black: rgb(black),
        red: rgb(red),
        green: rgb(green),
        yellow: rgb(yellow),
        blue: rgb(blue),
        magenta: rgb(magenta),
        cyan: rgb(cyan),
        white: rgb(white),
        bright_black: rgb(bright_black),
        bright_red: rgb(bright_red),
        bright_green: rgb(bright_green),
        bright_yellow: rgb(bright_yellow),
        bright_blue: rgb(bright_blue),
        bright_magenta: rgb(bright_magenta),
        bright_cyan: rgb(bright_cyan),
        bright_white: rgb(bright_white),
    }
}

fn onedark() -> Palette {
    hex_palette(
        0x282c34, 0xabb2bf, 0x1e2127, 0xe06c75, 0x98c379, 0xd19a66, 0x61afef, 0xc678dd, 0x56b6c2,
        0xabb2bf, 0x5c6370, 0xe06c75, 0x98c379, 0xd19a66, 0x61afef, 0xc678dd, 0x56b6c2, 0xffffff,
    )
}

fn dracula() -> Palette {
    hex_palette(
        0x282a36, 0xf8f8f2, 0x000000, 0xff5555, 0x50fa7b, 0xf1fa8c, 0xbd93f9, 0xff79c6, 0x8be9fd,
        0xbbbbbb, 0x555555, 0xff5555, 0x50fa7b, 0xf1fa8c, 0xcaa9fa, 0xff79c6, 0x8be9fd, 0xffffff,
    )
}

fn everforest() -> Palette {
    hex_palette(
        0x2d353b, 0xd3c6aa, 0x475258, 0xe67e80, 0xa7c080, 0xdbbc7f, 0x7fbbb3, 0xd699b6, 0x83c092,
        0xd3c6aa, 0x475258, 0xe67e80, 0xa7c080, 0xdbbc7f, 0x7fbbb3, 0xd699b6, 0x83c092, 0xd3c6aa,
    )
}

fn tokyo_night() -> Palette {
    hex_palette(
        0x1a1b26, 0xc0caf5, 0x15161e, 0xf7768e, 0x9ece6a, 0xe0af68, 0x7aa2f7, 0xbb9af7, 0x7dcfff,
        0xa9b1d6, 0x414868, 0xff899d, 0x9fe044, 0xfaba4a, 0x8db0ff, 0xc7a9ff, 0xa4daff, 0xc0caf5,
    )
}

fn catppuccin_mocha() -> Palette {
    hex_palette(
        0x1e1e2e, 0xcdd6f4, 0x45475a, 0xf38ba8, 0xa6e3a1, 0xf9e2af, 0x89b4fa, 0xf5c2e7, 0x94e2d5,
        0xbac2de, 0x585b70, 0xf38ba8, 0xa6e3a1, 0xf9e2af, 0x89b4fa, 0xf5c2e7, 0x94e2d5, 0xa6adc8,
    )
}

fn gruvbox_dark() -> Palette {
    hex_palette(
        0x282828, 0xebdbb2, 0x282828, 0xcc241d, 0x98971a, 0xd79921, 0x458588, 0xb16286, 0x689d6a,
        0xa89984, 0x928374, 0xfb4934, 0xb8bb26, 0xfabd2f, 0x83a598, 0xd3869b, 0x8ec07c, 0xebdbb2,
    )
}

fn nord() -> Palette {
    hex_palette(
        0x2e3440, 0xd8dee9, 0x3b4252, 0xbf616a, 0xa3be8c, 0xebcb8b, 0x81a1c1, 0xb48ead, 0x88c0d0,
        0xe5e9f0, 0x4c566a, 0xbf616a, 0xa3be8c, 0xebcb8b, 0x81a1c1, 0xb48ead, 0x8fbcbb, 0xeceff4,
    )
}

fn kanagawa() -> Palette {
    hex_palette(
        0x1f1f28, 0xdcd7ba, 0x090618, 0xc34043, 0x76946a, 0xc0a36e, 0x7e9cd8, 0x957fb8, 0x6a9589,
        0xc8c093, 0x727169, 0xe82424, 0x98bb6c, 0xe6c384, 0x7fb4ca, 0x938aa9, 0x7aa89f, 0xdcd7ba,
    )
}

pub fn palette_by_name(raw: &str) -> Option<Palette> {
    Some(match resolve_theme_name(raw)? {
        "default" => Palette::default_ansi(),
        "onedark" => onedark(),
        "dracula" => dracula(),
        "everforest" => everforest(),
        "tokyo-night" => tokyo_night(),
        "catppuccin-mocha" => catppuccin_mocha(),
        "gruvbox-dark" => gruvbox_dark(),
        "nord" => nord(),
        "kanagawa" => kanagawa(),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Color;

    #[test]
    fn unknown_name_is_none() {
        assert!(palette_by_name("not-a-theme").is_none());
    }

    #[test]
    fn default_is_ansi_reset_canvas() {
        let p = palette_by_name("builtin").unwrap();
        assert_eq!(p.background, Color::Reset);
        assert_eq!(p.cyan, Color::Cyan);
    }

    #[test]
    fn kanagawa_pins_official_wave_hex() {
        let p = palette_by_name("kanagawa").unwrap();
        assert_eq!(p.background, Color::Rgb(0x1f, 0x1f, 0x28));
        assert_eq!(p.yellow, Color::Rgb(0xc0, 0xa3, 0x6e));
    }

    #[test]
    fn mocha_alias_is_catppuccin_mocha() {
        let a = palette_by_name("mocha").unwrap();
        let b = palette_by_name("catppuccin-mocha").unwrap();
        assert_eq!(a, b);
        assert_eq!(a.background, Color::Rgb(0x1e, 0x1e, 0x2e));
    }
}
