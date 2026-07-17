//! Color data shared between the CLI's ANSI text output (`formatter.rs`,
//! via the `colored` crate) and `aloggrep-tui`'s ratatui rendering. Plain
//! RGB/enum data with no dependency on either rendering crate, so both
//! sides stay visually in sync from one source of truth.

use crate::parser::Level;

pub type Rgb = (u8, u8, u8);

/// Timestamp/pid/tid/separator tint.
pub const MUTED: Rgb = (140, 140, 140);
/// Package name tint.
pub const PKG: Rgb = (180, 180, 100);
/// Highlight color for the active filter chain's own tag/msg patterns.
pub const FILTER_MATCH: Rgb = (180, 140, 50);
/// Background for the `V` level badge.
pub const VERBOSE_BG: Rgb = (100, 100, 100);

/// 8-color reading-friendly progressive palette for `--highlight` / TUI
/// search chips: (background, is_foreground_black). Hue steps ~40–45°;
/// mid saturation so multiple chips stay distinguishable without neon glare.
pub const USER_HIGHLIGHT: [(Rgb, bool); 8] = [
    ((220, 180, 60), true),   // amber
    ((230, 150, 90), true),   // peach
    ((210, 100, 100), false), // coral
    ((190, 90, 140), false),  // rose
    ((150, 110, 200), false), // lilac
    ((80, 140, 210), false),  // sky
    ((60, 170, 160), true),   // teal
    ((100, 190, 120), true),  // mint
];
/// Named badge colors for the level indicator (`" E "` etc.), one variant
/// per distinct visual treatment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Badge {
    Gray,
    Blue,
    Green,
    Yellow,
    Red,
    RedBold,
}

pub fn level_badge(level: Level) -> Badge {
    match level {
        Level::V => Badge::Gray,
        Level::D => Badge::Blue,
        Level::I => Badge::Green,
        Level::W => Badge::Yellow,
        Level::E => Badge::Red,
        Level::F => Badge::RedBold,
    }
}
