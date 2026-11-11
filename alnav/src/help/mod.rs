//! Focus-aware keybinding hints for the status bar and Help panel.
//!
//! Status bar uses [`status_hint_entries`] (idle LogList/Strip are curated
//! 1–2 keys; pending and modal surfaces keep the full [`context_entries`]
//! set). The `?` Help panel is two-level: Home (short Active + TOC) and
//! seven zone pages. Copy for contracts and key tables lives here.
//! Rendering is dim keys + normal labels with spacing (no `:` / `|`
//! separators).
//!
//! Key strings come from [`App::keymap`]; labels/details stay in this module.

use crate::app::{App, Focus, Surface};

mod hints;
mod pages;
mod search;
#[cfg(test)]
mod tests;

pub use hints::*;
pub use pages::*;
pub use search::*;

/// Minimum remaining character budget before we bother showing help.
pub const MIN_HELP_WIDTH: usize = 8;

/// Shared `J`/`K` step for LogList cursor movement and Help panel scroll.
pub const FAST_SCROLL_STEP: isize = 7;

/// Whether Help may open for the current app state.
pub fn help_available(app: &App) -> bool {
    matches!(app.surface(), Surface::Main)
        && app.dialog().is_none()
        && !app.pending.is_armed()
        && matches!(
            app.focus,
            Focus::LogList | Focus::ChipStrip | Focus::ExcludeStrip | Focus::HighlightStrip
        )
}
