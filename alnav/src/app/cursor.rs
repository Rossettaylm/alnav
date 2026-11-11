//! LogList cursor motion, follow mode, visual selection, and severe/highlight match jumps.

use super::Pending;
use super::{App, FindJumpResult};

/// Max synchronous `row_at` parses per `find_severe` keypress in File mode
/// when the severe cache has not yet been filled (prefetch miss).
const SEVERE_SYNC_PARSE_BUDGET: usize = 256;

impl App {
    /// Toggle LogList display density (`w`): multi-line wrap ↔ single-line
    /// collapsed. Session-only; does not touch `following`/`cursor`/`list_offset`.
    pub fn toggle_collapsed_view(&mut self) {
        self.collapsed_view = !self.collapsed_view;
    }

    pub fn move_cursor(&mut self, delta: isize) {
        if self.index.visible.is_empty() {
            return;
        }
        let new = self.cursor as isize + delta;
        self.cursor = new.clamp(0, self.index.visible.len() as isize - 1) as usize;
        self.match_stats_stale = true;
    }

    pub fn jump_top(&mut self) {
        self.cursor = 0;
        self.match_stats_stale = true;
    }

    pub fn jump_bottom(&mut self) {
        self.cursor = self.index.visible.len().saturating_sub(1);
        self.match_stats_stale = true;
    }

    /// Call after any new rows are appended in `drain`/`push_row`'s path: if
    /// following, keep the cursor pinned to the last visible row.
    pub fn follow_tick(&mut self) {
        if self.following {
            self.jump_bottom();
        }
    }

    /// Manual cursor movement pauses following, then auto-resumes if the
    /// cursor lands on the last visible row (`j`/`J`/wheel/page/`G` path).
    /// Esc on LogList (also Visual Esc / successful filter-group submit)
    /// still calls [`Self::resume_following`] directly.
    pub fn move_cursor_manual(&mut self, delta: isize) {
        self.following = false;
        self.move_cursor(delta);
        self.maybe_follow_at_bottom();
    }

    /// If the cursor is on the last visible row, pin and resume following.
    pub fn maybe_follow_at_bottom(&mut self) {
        if self.index.visible.is_empty() {
            return;
        }
        if self.cursor == self.index.visible.len() - 1 {
            self.resume_following();
        }
    }

    /// Pin to bottom and resume live follow (Esc on LogList / Visual Esc /
    /// filter-group submit / land on bottom via manual move or `G`).
    pub fn resume_following(&mut self) {
        self.following = true;
        self.jump_bottom();
    }

    /// Inclusive `[lo, hi]` range over `visible` indices while in visual-line
    /// mode; `None` when not selecting.
    pub fn selection_range(&self) -> Option<(usize, usize)> {
        let anchor = self.visual_anchor?;
        if self.index.visible.is_empty() {
            return None;
        }
        let cur = self.cursor.min(self.index.visible.len() - 1);
        let anchor = anchor.min(self.index.visible.len() - 1);
        Some((anchor.min(cur), anchor.max(cur)))
    }

    pub fn enter_visual_line(&mut self) {
        if self.index.visible.is_empty() {
            return;
        }
        if self.pending != Pending::StripD {
            self.pending = Pending::None;
        }
        self.following = false;
        self.visual_anchor = Some(self.cursor);
    }

    pub fn clear_visual(&mut self) {
        self.visual_anchor = None;
    }

    /// Probe whether visible slot `idx` is severe. `None` = budget abort (flash set).
    fn severe_at_visible(&mut self, idx: usize, sync_parses: &mut usize) -> Option<bool> {
        if self.index.store.is_file() {
            let src = self.source_idx_for_visible(idx)?;
            match self
                .index
                .store
                .as_file()
                .and_then(|f| f.severe_cached(src))
            {
                Some(v) => Some(v),
                None => {
                    if *sync_parses >= SEVERE_SYNC_PARSE_BUDGET {
                        self.set_flash("SEVERE…");
                        return None;
                    }
                    *sync_parses += 1;
                    Some(self.row_at(idx).is_some_and(|r| r.severe))
                }
            }
        } else {
            Some(self.row_at(idx).is_some_and(|r| r.severe))
        }
    }

    /// Jump to the next (`dir > 0`) or previous (`dir < 0`) severe visible row
    /// (level E/F or crash). No wrapscan — at the last/first hit returns
    /// [`FindJumpResult::NoMore`]. Independent of search.
    ///
    /// File mode prefers the severe cache / prefetch; synchronous `row_at` is
    /// budget-capped so a keypress never full-scans a huge visible set.
    pub fn find_severe(&mut self, dir: i8) -> FindJumpResult {
        let n = self.index.visible.len();
        if n == 0 {
            return FindJumpResult::None;
        }
        let mut sync_parses = 0usize;
        let cursor = self.cursor;

        if dir >= 0 {
            for idx in (cursor + 1)..n {
                match self.severe_at_visible(idx, &mut sync_parses) {
                    Some(true) => {
                        self.following = false;
                        self.cursor = idx;
                        self.match_stats_stale = true;
                        return FindJumpResult::Moved;
                    }
                    Some(false) => {}
                    None => return FindJumpResult::None,
                }
            }
        } else {
            for idx in (0..cursor).rev() {
                match self.severe_at_visible(idx, &mut sync_parses) {
                    Some(true) => {
                        self.following = false;
                        self.cursor = idx;
                        self.match_stats_stale = true;
                        return FindJumpResult::Moved;
                    }
                    Some(false) => {}
                    None => return FindJumpResult::None,
                }
            }
        }

        // Already on a hit → NoMore (avoids a full re-probe / budget abort).
        match self.severe_at_visible(cursor, &mut sync_parses) {
            Some(true) => return FindJumpResult::NoMore,
            Some(false) => {}
            None => return FindJumpResult::None,
        }
        for idx in 0..n {
            if idx == cursor {
                continue;
            }
            match self.severe_at_visible(idx, &mut sync_parses) {
                Some(true) => return FindJumpResult::NoMore,
                Some(false) => {}
                None => return FindJumpResult::None,
            }
        }
        FindJumpResult::None
    }

    /// Jump to the next (`dir > 0`) or previous (`dir < 0`) visible row whose
    /// tag or msg matches the globally active search group. No wrapscan.
    pub fn find_match(&mut self, dir: i8) -> FindJumpResult {
        let Some(active_idx) = self.active_highlight else {
            return FindJumpResult::None;
        };
        if !self
            .highlight_groups
            .groups
            .get(active_idx)
            .is_some_and(|g| g.enabled)
        {
            return FindJumpResult::None;
        }
        if self.index.store.is_file() {
            return match self.highlight_scan.find_next(self.cursor, dir) {
                Some(idx) => {
                    self.following = false;
                    self.cursor = idx;
                    self.match_stats_stale = true;
                    FindJumpResult::Moved
                }
                None if self.highlight_scan.hits.is_empty() => FindJumpResult::None,
                None => FindJumpResult::NoMore,
            };
        }
        let n = self.index.visible.len();
        if n == 0 {
            return FindJumpResult::None;
        }
        let cursor = self.cursor;
        if dir >= 0 {
            for idx in (cursor + 1)..n {
                if self.visible_slot_matches_active(idx, active_idx) {
                    self.following = false;
                    self.cursor = idx;
                    self.match_stats_stale = true;
                    return FindJumpResult::Moved;
                }
            }
        } else {
            for idx in (0..cursor).rev() {
                if self.visible_slot_matches_active(idx, active_idx) {
                    self.following = false;
                    self.cursor = idx;
                    self.match_stats_stale = true;
                    return FindJumpResult::Moved;
                }
            }
        }
        if (0..n).any(|idx| self.visible_slot_matches_active(idx, active_idx)) {
            FindJumpResult::NoMore
        } else {
            FindJumpResult::None
        }
    }

    fn visible_slot_matches_active(&self, idx: usize, active_idx: usize) -> bool {
        let Some(row) = self.row_at(idx) else {
            return false;
        };
        self.highlight_groups.groups[active_idx].matches_entry(&row)
    }

    /// Jump to the first visible row matching search group `group_idx`.
    /// Used after committing a search (or re-submitting a duplicate).
    pub fn jump_first_match_of(&mut self, group_idx: usize) -> bool {
        let Some(group) = self.highlight_groups.groups.get(group_idx) else {
            return false;
        };
        if !group.enabled {
            return false;
        }
        if self.index.store.is_file() {
            if let Some(h) = self.highlight_scan.first_hit() {
                self.following = false;
                self.cursor = h;
                self.match_stats_stale = true;
                self.pending_jump_first = None;
                return true;
            }
            if self.highlight_scan.done {
                return false;
            }
            // Scan still running — jump when the first hit arrives.
            self.following = false;
            self.pending_jump_first = Some(group_idx);
            return true;
        }
        for idx in 0..self.index.visible.len() {
            let Some(row) = self.row_at(idx) else {
                continue;
            };
            if group.matches_entry(&row) {
                self.following = false;
                self.cursor = idx;
                self.match_stats_stale = true;
                return true;
            }
        }
        false
    }

    /// Jump to the first visible row matching the newest search group.
    pub fn jump_first_match(&mut self) -> bool {
        let Some(group_idx) = self.highlight_groups.groups.len().checked_sub(1) else {
            return false;
        };
        self.jump_first_match_of(group_idx)
    }

    /// Search hit position among visible rows for the globally active group.
    /// Recomputes lazily when the stale flag is set OR the cursor moved since
    /// the last computation (handles direct `cursor` field writes in tests).
    pub fn highlight_match_stats(&mut self) -> Option<(Option<usize>, usize)> {
        if self.match_stats_stale || self.cursor != self.match_stats_cursor {
            self.match_stats_stale = false;
            self.match_stats_cursor = self.cursor;
            self.cached_match_stats = self.compute_match_stats_inner();
        }
        self.cached_match_stats
    }

    /// Eagerly recompute and cache highlight match stats if the stale flag is set.
    /// File mode reads the async hit index (O(log n)); Stream may O(visible) parse.
    pub fn recompute_match_stats_if_stale(&mut self) {
        if self.match_stats_stale || self.cursor != self.match_stats_cursor {
            self.match_stats_stale = false;
            self.match_stats_cursor = self.cursor;
            self.cached_match_stats = self.compute_match_stats_inner();
        }
    }

    fn compute_match_stats_inner(&self) -> Option<(Option<usize>, usize)> {
        let Some(_group) = self.active_highlight_group() else {
            return None;
        };
        if self.index.store.is_file() {
            return Some(self.highlight_scan.match_stats(self.cursor));
        }
        let group = self.active_highlight_group()?;
        let mut total = 0usize;
        let mut current = None;
        for idx in 0..self.index.visible.len() {
            let Some(row) = self.row_at(idx) else {
                continue;
            };
            if group.matches_entry(&row) {
                total += 1;
                if idx == self.cursor {
                    current = Some(total);
                }
            }
        }
        Some((current, total))
    }
}
