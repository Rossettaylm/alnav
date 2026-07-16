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

/// 8-color palette cycled through for `--highlight`/ad-hoc keyword matches:
/// (background, is_foreground_black).
pub const USER_HIGHLIGHT: [(Rgb, bool); 8] = [
    ((255, 255, 0), true),
    ((0, 255, 128), true),
    ((60, 120, 255), false),
    ((255, 80, 80), false),
    ((200, 100, 255), false),
    ((0, 220, 220), true),
    ((255, 165, 0), true),
    ((255, 150, 200), true),
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
