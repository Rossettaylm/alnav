//! Terminal UI rendering: shared constants, block helpers and the `ui::*` re-export surface.

mod candidates;
mod dashboard;
mod entry;
mod layout;
mod log_list;
mod panels;
mod picker;
mod status;
mod strips;
#[cfg(test)]
mod tests;
mod text;

use crate::theme;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::{Block, BorderType, Borders, Clear};
use ratatui::Frame;

pub use candidates::*;
pub use dashboard::*;
pub use layout::*;
pub use log_list::*;
pub use panels::compare::*;
pub use panels::detail::*;
pub use panels::help::*;
pub use panels::hist::*;
pub use panels::input::*;
pub use panels::palette::*;
pub use panels::source::*;
pub use panels::summary::*;
pub use panels::time::*;
pub use picker::*;
pub use status::*;
pub use strips::*;
pub use text::*;

/// Gap between adjacent pills inside a group.
const PILL_GAP: u16 = 1;

/// Rounded search input height at the bottom of the left pane.
const PICKER_SEARCH_HEIGHT: u16 = 3;

/// Per-row action icon for candidate lists (F3). `None` = no icon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionKind {
    None,
    Jump,
    Toggle { enabled: bool },
}

impl ActionKind {
    fn icon(self) -> &'static str {
        match self {
            ActionKind::None => "",
            ActionKind::Jump => theme::GLYPH_ACTION_JUMP,
            ActionKind::Toggle { enabled: true } => theme::GLYPH_ACTION_TOGGLE_ON,
            ActionKind::Toggle { enabled: false } => theme::GLYPH_ACTION_TOGGLE_OFF,
        }
    }

    fn icon_style(self) -> Style {
        match self {
            ActionKind::None => Style::default(),
            ActionKind::Jump => Style::default().fg(theme::accent()),
            ActionKind::Toggle { enabled: true } => Style::default().fg(theme::success()),
            ActionKind::Toggle { enabled: false } => theme::disabled_chip_style(),
        }
    }
}

fn rounded_block(title: Line<'static>, active: bool) -> Block<'static> {
    Block::new()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(theme::border_style(active))
        .style(theme::canvas_style())
        .title(title)
}

/// Top/bottom-only divider block (Q3 path B: weakened borders). Uses
/// box-drawing `─` (U+2500) for horizontal rules; no left/right borders,
/// giving the inner content 2 extra columns vs `rounded_block`.
fn divider_block(title: Line<'static>, active: bool) -> Block<'static> {
    Block::new()
        .borders(Borders::TOP | Borders::BOTTOM)
        .border_type(BorderType::Plain)
        .border_style(theme::border_style(active))
        .style(theme::canvas_style())
        .title(title)
}

/// Rounded four-sided popup shell with a glyph-prefixed plain title.
/// Returns the inner content rect.
fn popup_block(title: &str) -> Block<'static> {
    rounded_block(
        theme::plain_title(theme::GLYPH_TITLE_PICKER, title, true),
        true,
    )
}

fn clear_to_canvas(frame: &mut Frame, area: Rect) {
    frame.render_widget(Clear, area);
    frame.render_widget(Block::default().style(theme::canvas_style()), area);
}

/// Clear + rounded full-border shell (dim accent). Returns the inner content rect.
pub fn render_modal_shell(title: &str, frame: &mut Frame, area: Rect) -> Rect {
    render_modal_shell_glyph(theme::GLYPH_TITLE_PICKER, title, frame, area)
}

/// Like [`render_modal_shell`] but with an explicit title glyph.
pub fn render_modal_shell_glyph(
    glyph: &'static str,
    title: &str,
    frame: &mut Frame,
    area: Rect,
) -> Rect {
    clear_to_canvas(frame, area);
    let block = rounded_block(theme::plain_title(glyph, title, true), true);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    clear_to_canvas(frame, inner);
    inner
}
