//! Nerdfont semantic glyph constants, Dashboard logo and chip-field icons.

use crate::input::ChipField;

// ---------------------------------------------------------------------------
// Nerdfont semantic glyphs (hard dependency — no runtime fallback).
// All UI iconography must reference these constants; `ui.rs` MUST NOT inline
// glyph literals. See prd.md R4 / design.md D1 for the rationale and table.
// ---------------------------------------------------------------------------

/// Manage/search prompt (Create / Bookmark / Unified). nf-fa-search — was
/// `\u{f0b7}` (fa-circle) which rendered cramped against the draft text.
pub const GLYPH_MODE_MANAGE: &str = "\u{f002}"; // nf-fa-search
pub const GLYPH_MODE_NEW: &str = "\u{f0fe}"; // nf-fa-plus_square
pub const GLYPH_MODE_EDIT: &str = "\u{f044}"; // nf-fa-pencil
pub const GLYPH_CARET_SEL: &str = "\u{f0da}"; //
pub const GLYPH_TITLE_PICKER: &str = "\u{f002}"; //
pub const GLYPH_TITLE_LOG: &str = "\u{f0c5}"; //
pub const GLYPH_TITLE_FILTER: &str = "\u{f0b0}"; //
pub const GLYPH_TITLE_EXCLUDE: &str = "\u{f056}"; //
pub const GLYPH_TITLE_HIGHLIGHT: &str = "\u{f0e0}"; //
pub const GLYPH_GROUP_ON: &str = "\u{f192}"; //
pub const GLYPH_GROUP_OFF: &str = "\u{f10c}"; //
pub const GLYPH_BOOKMARK: &str = "\u{f02e}"; // nf-fa-bookmark
/// Log-top compare-tray summary and panel stale mark (not the nerdfont pin).
pub const GLYPH_BOOKMARK_PIN: &str = "★";
pub const GLYPH_BOOKMARK_STALE_MARK: &str = "☆";
pub const GLYPH_COMPARE_UNTIMED: &str = "—";
pub const GLYPH_ACTION_JUMP: &str = "\u{f061}"; //  nf-fa-arrow_right
pub const GLYPH_ACTION_TOGGLE_ON: &str = "\u{f205}"; //  nf-fa-toggle_on
pub const GLYPH_ACTION_TOGGLE_OFF: &str = "\u{f204}"; //  nf-fa-toggle_off
pub const GLYPH_LOCK: &str = "\u{f023}"; //
pub const GLYPH_DISCONNECT: &str = "\u{f1e6}"; // nf-fa-plug (f127 chain-broken overflows right in non-Mono NF)
pub const GLYPH_TIME: &str = "\u{f017}"; // nf-fa-clock_o
pub const GLYPH_VIEW_FOCUS: &str = "\u{f06e}"; // nf-fa-eye
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
pub const GLYPH_HELP: &str = "\u{f059}"; // nf-fa-question_circle
pub const GLYPH_PROGRESS: &str = "\u{f110}"; // nf-fa-spinner
pub const GLYPH_SOURCE_HDC: &str = "\u{f10b}"; // nf-fa-mobile
pub const GLYPH_SOURCE_ADB: &str = "\u{f17b}"; // nf-fa-android
pub const GLYPH_SOURCE_OPEN_FILE: &str = "\u{f07c}"; // nf-fa-folder_open
pub const GLYPH_SOURCE_RECENT: &str = "\u{f1da}"; // nf-fa-history
pub const GLYPH_SOURCE_FILE: &str = "\u{f15b}"; // nf-fa-file
pub const GLYPH_SOURCE_DIR: &str = "\u{f07b}"; // nf-fa-folder
pub const GLYPH_TITLE_DASHBOARD: &str = "\u{f0e4}"; // nf-fa-tachometer
pub const GLYPH_TITLE_PALETTE: &str = "\u{f0ca}"; // nf-fa-list-ul (command palette)
pub const GLYPH_QUIT: &str = "\u{f08b}"; // nf-fa-sign_out

/// Six-line dashboard-nvim-style Unicode wordmark. Keeping this in the theme
/// module makes the startup branding a single visual asset alongside semantic
/// glyphs.
pub const DASHBOARD_LOGO_WIDTH: u16 = 43;
pub const DASHBOARD_LOGO: [&str; 6] = [
    " █████╗ ██╗     ███╗   ██╗ █████╗ ██╗   ██╗",
    "██╔══██╗██║     ████╗  ██║██╔══██╗██║   ██║",
    "███████║██║     ██╔██╗ ██║███████║██║   ██║",
    "██╔══██║██║     ██║╚██╗██║██╔══██║╚██╗ ██╔╝",
    "██║  ██║███████╗██║ ╚████║██║  ██║ ╚████╔╝ ",
    "╚═╝  ╚═╝╚══════╝╚═╝  ╚═══╝╚═╝  ╚═╝  ╚═══╝  ",
];

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
