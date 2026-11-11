//! Session bookmarks and the compare tray.

use super::{App, Focus};
use crate::bookmark::{AddError, Bookmark, ComparePanel, JumpResult};

impl App {
    /// Arm `m` operator-pending (M2 bookmarks).
    pub fn begin_bookmark_op(&mut self) {
        self.clear_visual();
        self.pending_yank = false;
        self.pending_d = false;
        self.pending_chip = false;
        self.pending_exclude = false;
        self.pending_lock = false;
        self.pending_time = false;
        self.pending_m = false;
        self.pending_leader = false;
        self.pending_m = true;
    }

    pub fn cancel_bookmark_op(&mut self) {
        self.pending_m = false;
        self.pending_leader = false;
    }

    /// `ma`: toggle pin on the current LogList row (snapshot copy).
    pub fn bookmark_add_current(&mut self) {
        self.pending_m = false;
        self.pending_leader = false;
        let Some(row) = self.current_row() else {
            self.set_flash("NO ROW");
            return;
        };
        let row_id = row.row_id;
        if self.bookmarks.contains_id(row_id) {
            self.bookmarks.remove_id(row_id);
            self.bookmark_row_ids.remove(&row_id);
            self.unpin_from_visible(row_id);
            self.set_flash("REMOVED");
            if self.bookmarks.is_empty() {
                self.close_compare_panel();
            } else if let Some(panel) = self.compare.as_mut() {
                panel.clamp_cursor(self.bookmarks.len());
            }
            return;
        }
        let bm = Bookmark::from_row((*row).clone());
        match self.bookmarks.try_add(bm) {
            Ok(()) => {
                self.bookmark_row_ids.insert(row_id);
                self.set_flash("BOOKMARKED");
            }
            Err(AddError::Duplicate) => self.set_flash("EXISTS"),
            Err(AddError::Full) => self.set_flash("BOOKMARKS FULL"),
        }
    }

    /// `mm`: open the compare panel, or flash `NO BOOKMARKS` when the tray is empty.
    pub fn open_compare_panel(&mut self) {
        self.pending_m = false;
        self.pending_leader = false;
        if self.bookmarks.is_empty() {
            self.set_flash("NO BOOKMARKS");
            return;
        }
        self.clear_visual();
        self.following = false;
        self.compare = Some(ComparePanel::new());
    }

    /// Close the compare panel without resuming follow. Clears panel pendings.
    pub fn close_compare_panel(&mut self) {
        self.compare = None;
    }

    fn compare_selected_storage_index(&self) -> Option<usize> {
        let panel = self.compare.as_ref()?;
        let sorted = self.bookmarks.sorted_indices();
        sorted.get(panel.cursor).copied()
    }

    pub fn compare_move(&mut self, delta: isize) {
        let len = self.bookmarks.len();
        if let Some(panel) = self.compare.as_mut() {
            panel.move_by(delta, len);
        }
    }

    pub fn compare_goto_first(&mut self) {
        if let Some(panel) = self.compare.as_mut() {
            panel.cursor = 0;
            panel.clear_pending();
        }
    }

    pub fn compare_goto_last(&mut self) {
        let last = self.bookmarks.len().saturating_sub(1);
        if let Some(panel) = self.compare.as_mut() {
            panel.cursor = last;
            panel.clear_pending();
        }
    }

    pub fn compare_yank_selected(&mut self) {
        let Some(idx) = self.compare_selected_storage_index() else {
            return;
        };
        let raw = self.bookmarks.items[idx].row.raw.clone();
        if let Some(panel) = self.compare.as_mut() {
            panel.clear_pending();
        }
        self.apply_yank(raw);
    }

    pub fn compare_delete_selected(&mut self) {
        let Some(idx) = self.compare_selected_storage_index() else {
            return;
        };
        self.delete_bookmark_at_index(idx);
        if self.bookmarks.is_empty() {
            self.close_compare_panel();
            return;
        }
        if let Some(panel) = self.compare.as_mut() {
            panel.clear_pending();
            panel.clamp_cursor(self.bookmarks.len());
        }
    }

    pub fn compare_jump_selected(&mut self) {
        let Some(idx) = self.compare_selected_storage_index() else {
            return;
        };
        let row_id = self.bookmarks.items[idx].row_id();
        match self.jump_to_bookmark(row_id) {
            JumpResult::Ok => {
                self.close_compare_panel();
                self.focus = Focus::LogList;
            }
            JumpResult::Filtered => self.set_flash("BOOKMARK NOT VISIBLE"),
            JumpResult::Evicted => self.set_flash("BOOKMARK EVICTED"),
        }
    }

    /// `md`: remove bookmark for current row.
    pub fn bookmark_remove_current(&mut self) {
        self.pending_m = false;
        self.pending_leader = false;
        let Some(row) = self.current_row() else {
            self.set_flash("NO ROW");
            return;
        };
        let row_id = row.row_id;
        if self.bookmarks.remove_id(row_id) {
            self.bookmark_row_ids.remove(&row_id);
            self.unpin_from_visible(row_id);
            self.set_flash("REMOVED");
        } else {
            self.set_flash("NOT BOOKMARKED");
        }
    }

    /// Jump to a bookmarked row_id; sets `following=false` on success.
    /// A bookmark is jumpable iff its row is still alive (`row_alive`).
    /// A row retained in `matched` (evicted from `rows`) is still jumpable.
    pub fn jump_to_bookmark(&mut self, row_id: u64) -> JumpResult {
        let Some(vis_idx) = self.visible_idx_for_row_id(row_id) else {
            return if self.row_alive(row_id) {
                JumpResult::Filtered
            } else {
                JumpResult::Evicted
            };
        };
        self.following = false;
        self.cursor = vis_idx;
        self.match_stats_stale = true;
        JumpResult::Ok
    }

    /// Whether `row_id` is still present in either ring buffer.
    pub fn bookmark_alive(&self, row_id: u64) -> bool {
        self.row_alive(row_id)
    }

    pub fn clear_bookmarks(&mut self) {
        self.bookmark_row_ids.clear();
        self.bookmarks.clear();
        self.close_compare_panel();
    }

    /// Delete a bookmark by index into `bookmarks.items`; keeps
    /// `bookmark_row_ids` in sync (F1). Returns false if index out of range.
    pub fn delete_bookmark_at_index(&mut self, index: usize) -> bool {
        let row_id = self.bookmarks.items.get(index).map(|b| b.row_id());
        if !self.bookmarks.delete_at(index) {
            return false;
        }
        if let Some(rid) = row_id {
            self.bookmark_row_ids.remove(&rid);
            self.unpin_from_visible(rid);
        }
        true
    }

    /// O(1) bookmark-row check for LogList bg (F1).
    pub fn is_bookmark_row(&self, row_id: u64) -> bool {
        self.bookmark_row_ids.contains(&row_id)
    }
}
