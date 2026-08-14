//! VS Code-style command palette session (query + selection). No paint, no dispatch.

use crate::text_field::TextField;

/// Max candidate rows painted at once (dropdown viewport).
pub const PALETTE_VISIBLE_ROWS: usize = 10;

/// In-session command palette (opened by `C-p` / [`crate::keymap::ActionId::GlobalCommandPalette`]).
#[derive(Debug, Clone)]
pub struct CommandPalette {
    pub query: TextField,
    pub selected: usize,
}

impl CommandPalette {
    pub fn new() -> Self {
        Self {
            query: TextField::new(),
            selected: 0,
        }
    }

    /// Clamp `selected` into `0..n` (`n == 0` → `0`).
    pub fn clamp_selected(&mut self, n: usize) {
        if n == 0 {
            self.selected = 0;
        } else {
            self.selected = self.selected.min(n - 1);
        }
    }

    /// Move the highlight by `delta` within `n` hits. No-op when `n == 0`.
    pub fn move_sel(&mut self, delta: isize, n: usize) {
        if n == 0 {
            self.selected = 0;
            return;
        }
        let next = self.selected as isize + delta;
        self.selected = next.clamp(0, n as isize - 1) as usize;
    }
}

impl Default for CommandPalette {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyCode;

    #[test]
    fn empty_query_starts_with_no_selection_shift() {
        let p = CommandPalette::new();
        assert!(p.query.is_empty());
        assert_eq!(p.selected, 0);
    }

    #[test]
    fn typing_k_does_not_move_selection() {
        let mut p = CommandPalette::new();
        p.selected = 2;
        assert!(crate::text_field::apply_key(
            &mut p.query,
            KeyCode::Char('k'),
            false
        ));
        assert_eq!(p.query.as_str(), "k");
        assert_eq!(p.selected, 2, "j/k type into the query; they do not move");
    }

    #[test]
    fn move_sel_clamps() {
        let mut p = CommandPalette::new();
        p.move_sel(1, 0);
        assert_eq!(p.selected, 0);
        p.move_sel(1, 3);
        assert_eq!(p.selected, 1);
        p.move_sel(10, 3);
        assert_eq!(p.selected, 2);
        p.move_sel(-10, 3);
        assert_eq!(p.selected, 0);
    }
}
