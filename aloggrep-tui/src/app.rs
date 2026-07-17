use std::collections::VecDeque;
use std::sync::mpsc::Receiver;

use crate::filter_model::GroupList;
use crate::model::EntryRow;
use crate::search_model::{SearchBox, SearchGroupList};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    ChipStrip,
    SearchStrip,
    LogList,
    Input,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Insert,
}

/// Which chip strip the shared `h`/`l`/`dd`/`di` ops target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StripKind {
    Filter,
    Search,
}

/// Second-key target for the `y` operator (`yy`/`yt`/`ym`/…).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum YankField {
    Raw,
    Tag,
    Msg,
    Pid,
    Tid,
    Level,
    Pkg,
    Timestamp,
}

impl YankField {
    pub fn from_char(c: char) -> Option<Self> {
        match c {
            'y' | 'r' => Some(Self::Raw),
            't' => Some(Self::Tag),
            'm' => Some(Self::Msg),
            'p' => Some(Self::Pid),
            'T' => Some(Self::Tid),
            'l' => Some(Self::Level),
            'g' => Some(Self::Pkg),
            's' => Some(Self::Timestamp),
            _ => None,
        }
    }
}

pub struct App {
    pub rows: VecDeque<EntryRow>,
    pub visible: Vec<usize>,
    pub groups: GroupList,
    pub search_groups: SearchGroupList,
    pub search_box: SearchBox,
    pub cursor: usize,
    pub max_lines: usize,
    pub should_quit: bool,
    pub focus: Focus,
    pub mode: Mode,
    pub group_cursor: usize,
    pub search_cursor: usize,
    /// Armed by first `d` on a chip strip; second `d` deletes, `i` toggles disable.
    pub pending_d: bool,
    pub pending_yank: bool,
    /// When `Some`, LogList is in visual-line mode; value is the anchor
    /// index into `visible` (same coordinate space as `cursor`).
    pub visual_anchor: Option<usize>,
    pub following: bool,
    pub list_offset: usize,
    /// Transient status-bar hint (`YANKED`, `VISUAL`, `y…`, errors).
    pub status_msg: Option<String>,
    /// Last text prepared for the clipboard (set even if clipboard I/O fails).
    pub last_yanked: Option<String>,
}

impl App {
    pub fn new(max_lines: usize) -> Self {
        Self {
            rows: VecDeque::new(),
            visible: Vec::new(),
            groups: GroupList::default(),
            search_groups: SearchGroupList::default(),
            search_box: SearchBox::default(),
            cursor: 0,
            max_lines,
            should_quit: false,
            focus: Focus::LogList,
            mode: Mode::Normal,
            group_cursor: 0,
            search_cursor: 0,
            pending_d: false,
            pending_yank: false,
            visual_anchor: None,
            following: true,
            list_offset: 0,
            status_msg: None,
            last_yanked: None,
        }
    }

    /// Drain everything currently available on the channel without blocking.
    /// Each new row is checked against the current filter in O(1) and, if
    /// visible, appended — no full rescan (see design doc "增量过滤").
    pub fn drain(&mut self, rx: &Receiver<EntryRow>) {
        while let Ok(row) = rx.try_recv() {
            self.push_row(row);
        }
    }

    fn push_row(&mut self, row: EntryRow) {
        if self.rows.len() >= self.max_lines {
            self.rows.pop_front();
            // `visible` stays sorted ascending, so only index 0 can ever need removing.
            let evicted_was_visible = self.visible.first() == Some(&0);
            if evicted_was_visible {
                self.visible.remove(0);
                // `list_offset` is an index into `visible`; dropping the front
                // item must shift the viewport with the content, otherwise the
                // list appears to scroll up even when no new visible rows arrive
                // (common under a tight filter while the ring buffer fills).
                if self.list_offset > 0 {
                    self.list_offset -= 1;
                }
            }
            for i in self.visible.iter_mut() {
                *i -= 1;
            }
            if evicted_was_visible && self.cursor > 0 {
                self.cursor -= 1;
            }
            // `visual_anchor` shares `cursor`'s coordinate space (index into
            // `visible`). Evicting the oldest visible row shifts that space.
            if evicted_was_visible {
                match self.visual_anchor {
                    Some(0) => self.visual_anchor = None,
                    Some(a) => self.visual_anchor = Some(a - 1),
                    None => {}
                }
            }
        }
        let matches = self.groups.matches(&row);
        self.rows.push_back(row);
        if matches {
            self.visible.push(self.rows.len() - 1);
        }
        self.follow_tick();
    }

    /// Full rescan, used when the filter groups themselves change.
    pub fn rebuild_visible(&mut self) {
        self.visible = self
            .rows
            .iter()
            .enumerate()
            .filter(|(_, row)| self.groups.matches(row))
            .map(|(i, _)| i)
            .collect();
        if self.following {
            self.jump_bottom();
        } else if self.cursor >= self.visible.len() {
            self.cursor = self.visible.len().saturating_sub(1);
        }
    }

    pub fn move_cursor(&mut self, delta: isize) {
        if self.visible.is_empty() {
            return;
        }
        let new = self.cursor as isize + delta;
        self.cursor = new.clamp(0, self.visible.len() as isize - 1) as usize;
    }

    pub fn jump_top(&mut self) {
        self.cursor = 0;
    }

    pub fn jump_bottom(&mut self) {
        self.cursor = self.visible.len().saturating_sub(1);
    }

    /// Call after any new rows are appended in `drain`/`push_row`'s path: if
    /// following, keep the cursor pinned to the last visible row.
    pub fn follow_tick(&mut self) {
        if self.following {
            self.jump_bottom();
        }
    }

    /// Any manual cursor movement pauses following. Resume only via Esc on
    /// LogList (also Visual Esc / successful filter-group submit).
    pub fn move_cursor_manual(&mut self, delta: isize) {
        self.following = false;
        self.move_cursor(delta);
    }

    /// Pin to bottom and resume live follow (Esc on LogList / Visual Esc /
    /// filter-group submit).
    pub fn resume_following(&mut self) {
        self.following = true;
        self.jump_bottom();
    }

    pub fn visible_rows(&self) -> impl Iterator<Item = &EntryRow> {
        self.visible.iter().map(move |&i| &self.rows[i])
    }

    pub fn cycle_focus_forward(&mut self) {
        self.focus = match self.focus {
            Focus::ChipStrip => Focus::SearchStrip,
            Focus::SearchStrip => Focus::LogList,
            Focus::LogList => Focus::Input,
            Focus::Input => Focus::ChipStrip,
        };
    }

    pub fn cycle_focus_backward(&mut self) {
        self.focus = match self.focus {
            Focus::ChipStrip => Focus::Input,
            Focus::SearchStrip => Focus::ChipStrip,
            Focus::LogList => Focus::SearchStrip,
            Focus::Input => Focus::LogList,
        };
    }

    fn strip_len(&self, kind: StripKind) -> usize {
        match kind {
            StripKind::Filter => self.groups.groups.len(),
            StripKind::Search => self.search_groups.groups.len(),
        }
    }

    fn strip_cursor_mut(&mut self, kind: StripKind) -> &mut usize {
        match kind {
            StripKind::Filter => &mut self.group_cursor,
            StripKind::Search => &mut self.search_cursor,
        }
    }

    pub fn move_strip_cursor(&mut self, kind: StripKind, delta: isize) {
        let len = self.strip_len(kind);
        if len == 0 {
            return;
        }
        let cursor = *self.strip_cursor_mut(kind);
        let new = (cursor as isize + delta).clamp(0, len as isize - 1) as usize;
        *self.strip_cursor_mut(kind) = new;
    }

    pub fn move_group_cursor(&mut self, delta: isize) {
        self.move_strip_cursor(StripKind::Filter, delta);
    }

    /// Delete the focused group on `kind`. Empty filter strip returns focus
    /// to LogList and rebuilds visible; empty search strip only clamps cursor.
    pub fn delete_focused_strip_group(&mut self, kind: StripKind) {
        let len = self.strip_len(kind);
        if len == 0 {
            return;
        }
        let cursor = *self.strip_cursor_mut(kind);
        match kind {
            StripKind::Filter => {
                self.groups.groups.remove(cursor);
                if self.group_cursor >= self.groups.groups.len() {
                    self.group_cursor = self.groups.groups.len().saturating_sub(1);
                }
                if self.groups.groups.is_empty() {
                    self.focus = Focus::LogList;
                }
                self.rebuild_visible();
            }
            StripKind::Search => {
                self.search_groups.groups.remove(cursor);
                if self.search_cursor >= self.search_groups.groups.len() {
                    self.search_cursor = self.search_groups.groups.len().saturating_sub(1);
                }
                if self.search_groups.groups.is_empty() {
                    self.focus = Focus::LogList;
                }
            }
        }
    }

    pub fn delete_focused_group(&mut self) {
        self.delete_focused_strip_group(StripKind::Filter);
    }

    /// Toggle `enabled` on the focused group (`di`). Does not change focus
    /// when all groups become disabled.
    pub fn toggle_disable_focused(&mut self, kind: StripKind) {
        let len = self.strip_len(kind);
        if len == 0 {
            return;
        }
        let cursor = *self.strip_cursor_mut(kind);
        match kind {
            StripKind::Filter => {
                let g = &mut self.groups.groups[cursor];
                g.enabled = !g.enabled;
                self.rebuild_visible();
            }
            StripKind::Search => {
                let g = &mut self.search_groups.groups[cursor];
                g.enabled = !g.enabled;
            }
        }
    }

    pub fn current_row(&self) -> Option<&EntryRow> {
        self.visible.get(self.cursor).map(|&i| &self.rows[i])
    }

    /// Inclusive `[lo, hi]` range over `visible` indices while in visual-line
    /// mode; `None` when not selecting.
    pub fn selection_range(&self) -> Option<(usize, usize)> {
        let anchor = self.visual_anchor?;
        if self.visible.is_empty() {
            return None;
        }
        let cur = self.cursor.min(self.visible.len() - 1);
        let anchor = anchor.min(self.visible.len() - 1);
        Some((anchor.min(cur), anchor.max(cur)))
    }

    pub fn enter_visual_line(&mut self) {
        if self.visible.is_empty() {
            return;
        }
        self.pending_yank = false;
        self.following = false;
        self.visual_anchor = Some(self.cursor);
        self.status_msg = Some("VISUAL".into());
    }

    pub fn clear_visual(&mut self) {
        self.visual_anchor = None;
        if self.status_msg.as_deref() == Some("VISUAL") {
            self.status_msg = None;
        }
    }

    pub fn field_text(row: &EntryRow, field: YankField) -> String {
        match field {
            YankField::Raw => row.raw.clone(),
            YankField::Tag => row.tag.clone(),
            YankField::Msg => row.msg.clone(),
            YankField::Pid => row.pid.clone(),
            YankField::Tid => row.tid.clone(),
            YankField::Level => row.level.as_char().to_string(),
            YankField::Pkg => row.pkg.clone(),
            YankField::Timestamp => row.timestamp.clone(),
        }
    }

    pub fn yank_field(&self, field: YankField) -> Option<String> {
        self.current_row().map(|row| Self::field_text(row, field))
    }

    /// Join `field` values for visible indices `lo..=hi` with newlines.
    pub fn yank_range(&self, lo: usize, hi: usize, field: YankField) -> Option<String> {
        if self.visible.is_empty() || lo > hi || hi >= self.visible.len() {
            return None;
        }
        let mut parts = Vec::with_capacity(hi - lo + 1);
        for vi in lo..=hi {
            let row = &self.rows[self.visible[vi]];
            parts.push(Self::field_text(row, field));
        }
        Some(parts.join("\n"))
    }

    pub fn record_yank(&mut self, text: String) {
        self.last_yanked = Some(text);
    }

    /// Jump to the next (`dir > 0`) or previous (`dir < 0`) visible row whose
    /// `msg` matches any enabled search pattern. Wraps like vim `wrapscan`.
    pub fn find_match(&mut self, dir: i8) -> bool {
        if self.search_groups.active_patterns().is_empty() {
            return false;
        }
        let n = self.visible.len();
        if n == 0 {
            return false;
        }
        let step: isize = if dir >= 0 { 1 } else { -1 };
        let start = self.cursor as isize;
        for offset in 1..=n as isize {
            let idx = (start + offset * step).rem_euclid(n as isize) as usize;
            let row = &self.rows[self.visible[idx]];
            if self.search_groups.any_match(&row.msg) {
                self.following = false;
                self.cursor = idx;
                return true;
            }
        }
        false
    }

    /// Jump to the first visible row matching search group `group_idx`.
    /// Used after committing a search (or re-submitting a duplicate).
    pub fn jump_first_match_of(&mut self, group_idx: usize) -> bool {
        let Some(group) = self.search_groups.groups.get(group_idx) else {
            return false;
        };
        if !group.enabled {
            return false;
        }
        for idx in 0..self.visible.len() {
            let row_idx = self.visible[idx];
            if group.matches_msg(&self.rows[row_idx].msg) {
                self.following = false;
                self.cursor = idx;
                return true;
            }
        }
        false
    }

    /// Jump to the first visible row matching the newest search group.
    pub fn jump_first_match(&mut self) -> bool {
        let Some(group_idx) = self.search_groups.groups.len().checked_sub(1) else {
            return false;
        };
        self.jump_first_match_of(group_idx)
    }

    /// Push a filter group unless an equivalent already exists. Returns whether pushed.
    pub fn push_filter_group(&mut self, group: crate::filter_model::Group) -> bool {
        if self.groups.groups.iter().any(|g| g.same_as(&group)) {
            return false;
        }
        self.groups.groups.push(group);
        true
    }

    /// Push a search group, or return the index of an existing equivalent.
    /// Caller always jumps to that group's first match.
    pub fn push_or_find_search_group(&mut self, group: crate::search_model::SearchGroup) -> usize {
        if let Some(idx) = self.search_groups.find_equivalent(&group.pattern) {
            return idx;
        }
        self.search_groups.groups.push(group);
        self.search_groups.groups.len() - 1
    }

    /// Search hit position among visible rows: `None` when no enabled pattern;
    /// otherwise `(current_1based_or_none, total_hits)`. `current` is `None`
    /// when the cursor is not on a matching row.
    pub fn search_match_stats(&self) -> Option<(Option<usize>, usize)> {
        if self.search_groups.active_patterns().is_empty() {
            return None;
        }
        let mut total = 0usize;
        let mut current = None;
        for (idx, &row_idx) in self.visible.iter().enumerate() {
            if self.search_groups.any_match(&self.rows[row_idx].msg) {
                total += 1;
                if idx == self.cursor {
                    current = Some(total);
                }
            }
        }
        Some((current, total))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filter_model::Group;
    use aloggrep::expr::Expr;
    use std::sync::mpsc;

    fn row(tag: &str) -> EntryRow {
        EntryRow::from_line(&format!("04-02 10:00:00.000  1  1 I {tag}   : m")).unwrap()
    }

    fn filter_group(label: &str, expr: Option<Expr>) -> Group {
        Group {
            label: label.into(),
            chips: Vec::new(),
            expr,
            time: None,
            enabled: true,
        }
    }

    #[test]
    fn test_drain_appends_visible_rows() {
        let mut app = App::new(100);
        let (tx, rx) = mpsc::channel();
        tx.send(row("A")).unwrap();
        tx.send(row("B")).unwrap();
        drop(tx);
        app.drain(&rx);
        assert_eq!(app.visible.len(), 2);
    }

    #[test]
    fn test_new_app_has_zero_list_offset() {
        let app = App::new(100);
        assert_eq!(app.list_offset, 0);
    }

    #[test]
    fn test_ring_buffer_evicts_oldest_and_shifts_indices() {
        let mut app = App::new(2);
        let (tx, rx) = mpsc::channel();
        tx.send(row("A")).unwrap();
        tx.send(row("B")).unwrap();
        tx.send(row("C")).unwrap();
        drop(tx);
        app.drain(&rx);
        assert_eq!(app.rows.len(), 2);
        assert_eq!(app.rows[0].tag, "B");
        assert_eq!(app.rows[1].tag, "C");
        assert_eq!(app.visible, vec![0, 1]);
    }

    #[test]
    fn test_evicting_visible_front_decrements_list_offset() {
        let mut app = App::new(3);
        app.groups = GroupList {
            groups: vec![filter_group(
                "keep-A",
                Some(Expr::parse("tag~A", false).unwrap()),
            )],
        };
        let (tx, rx) = mpsc::channel();
        // Two matching rows fill visible; then non-matching rows churn the ring
        // until the oldest visible (A0) is evicted — list_offset must track.
        tx.send(row("A")).unwrap(); // A0
        tx.send(row("A")).unwrap(); // A1
        tx.send(row("X")).unwrap(); // fills buffer: [A0,A1,X]
        drop(tx);
        app.drain(&rx);
        assert_eq!(app.visible.len(), 2);
        app.following = false;
        app.list_offset = 1;
        app.cursor = 1;

        let (tx2, rx2) = mpsc::channel();
        tx2.send(row("Y")).unwrap(); // evict A0 → rows [A1,X,Y], visible drops front
        drop(tx2);
        app.drain(&rx2);

        assert_eq!(app.visible.len(), 1);
        assert_eq!(app.list_offset, 0, "viewport must shift with front eviction");
        assert_eq!(app.cursor, 0);
    }

    #[test]
    fn test_move_cursor_clamps() {
        let mut app = App::new(100);
        let (tx, rx) = mpsc::channel();
        tx.send(row("A")).unwrap();
        tx.send(row("B")).unwrap();
        drop(tx);
        app.drain(&rx);
        app.move_cursor(-5);
        assert_eq!(app.cursor, 0);
        app.move_cursor(5);
        assert_eq!(app.cursor, 1);
    }

    #[test]
    fn test_move_cursor_manual_large_delta_clamps_like_paging() {
        let mut app = App::new(100);
        let (tx, rx) = mpsc::channel();
        tx.send(row("A")).unwrap();
        tx.send(row("B")).unwrap();
        drop(tx);
        app.drain(&rx);
        app.move_cursor_manual(-10); // simulates Ctrl-u paging past the top
        assert_eq!(app.cursor, 0);
        assert!(!app.following, "negative delta should pause following");
        app.move_cursor_manual(10); // simulates Ctrl-d paging past the bottom
        assert_eq!(app.cursor, 1);
    }

    #[test]
    fn test_cursor_unaffected_when_evicted_row_was_already_filtered_out() {
        let mut app = App::new(3);
        app.groups = GroupList {
            groups: vec![filter_group("x", Some(Expr::parse("tag~X", false).unwrap()))],
        };
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(row("N1")).unwrap(); // filtered out, not in `visible`
        tx.send(row("X1")).unwrap();
        tx.send(row("X2")).unwrap();
        drop(tx);
        app.drain(&rx);
        assert_eq!(app.visible, vec![1, 2]);
        app.cursor = 1; // pointing at X2
        app.following = false;
        let selected_tag_before = app.rows[app.visible[app.cursor]].tag.clone();

        let (tx2, rx2) = std::sync::mpsc::channel();
        tx2.send(row("X3")).unwrap(); // triggers eviction of N1
        drop(tx2);
        app.drain(&rx2);

        let selected_tag_after = app.rows[app.visible[app.cursor]].tag.clone();
        assert_eq!(selected_tag_before, selected_tag_after, "cursor should still point at the same logical row");
        assert_eq!(selected_tag_after, "X2");
    }
}

#[cfg(test)]
mod focus_tests {
    use super::*;
    use crate::filter_model::Group;
    use aloggrep::expr::Expr;

    fn g(label: &str) -> Group {
        Group {
            label: label.into(),
            chips: Vec::new(),
            expr: None,
            time: None,
            enabled: true,
        }
    }

    #[test]
    fn test_cycle_focus_forward_wraps() {
        let mut app = App::new(100);
        assert_eq!(app.focus, Focus::LogList);
        app.cycle_focus_forward();
        assert_eq!(app.focus, Focus::Input);
        app.cycle_focus_forward();
        assert_eq!(app.focus, Focus::ChipStrip);
        app.cycle_focus_forward();
        assert_eq!(app.focus, Focus::SearchStrip);
        app.cycle_focus_forward();
        assert_eq!(app.focus, Focus::LogList);
    }

    #[test]
    fn test_delete_focused_group_removes_and_rescans() {
        let mut app = App::new(100);
        app.groups.groups.push(g("g0"));
        app.groups.groups.push(g("g1"));
        app.group_cursor = 0;
        app.delete_focused_group();
        assert_eq!(app.groups.groups.len(), 1);
        assert_eq!(app.groups.groups[0].label, "g1");
    }

    #[test]
    fn test_delete_focused_group_returns_focus_to_loglist_when_list_becomes_empty() {
        let mut app = App::new(100);
        app.groups.groups.push(g("g0"));
        app.focus = Focus::ChipStrip;
        app.delete_focused_group();
        assert!(app.groups.groups.is_empty());
        assert_eq!(app.focus, Focus::LogList);
    }

    #[test]
    fn test_delete_focused_group_keeps_focus_when_groups_remain() {
        let mut app = App::new(100);
        app.groups.groups.push(g("g0"));
        app.groups.groups.push(g("g1"));
        app.focus = Focus::ChipStrip;
        app.delete_focused_group();
        assert!(!app.groups.groups.is_empty());
        assert_eq!(app.focus, Focus::ChipStrip, "focus should stay put while groups remain");
    }

    #[test]
    fn test_move_group_cursor_clamps() {
        let mut app = App::new(100);
        app.groups.groups.push(g("g0"));
        app.move_group_cursor(-5);
        assert_eq!(app.group_cursor, 0);
        app.move_group_cursor(5);
        assert_eq!(app.group_cursor, 0);
    }

    #[test]
    fn test_toggle_disable_filter_rebuilds_visible() {
        let mut app = App::new(100);
        app.groups.groups.push(Group {
            label: "a".into(),
            chips: Vec::new(),
            expr: Some(Expr::parse("tag~A", false).unwrap()),
            time: None,
            enabled: true,
        });
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(EntryRow::from_line("04-02 10:00:00.000  1  1 I A   : m").unwrap()).unwrap();
        tx.send(EntryRow::from_line("04-02 10:00:00.000  1  1 I B   : m").unwrap()).unwrap();
        drop(tx);
        app.drain(&rx);
        assert_eq!(app.visible.len(), 1);
        app.toggle_disable_focused(StripKind::Filter);
        assert!(!app.groups.groups[0].enabled);
        assert_eq!(app.visible.len(), 2, "disabled-only list ≡ empty filter");
    }
}

#[cfg(test)]
mod follow_tests {
    use super::*;
    use crate::filter_model::Group;
    use aloggrep::expr::Expr;
    use std::sync::mpsc;

    fn row(tag: &str) -> EntryRow {
        EntryRow::from_line(&format!("04-02 10:00:00.000  1  1 I {tag}   : m")).unwrap()
    }

    #[test]
    fn test_follow_pins_cursor_to_latest() {
        let mut app = App::new(100);
        let (tx, rx) = mpsc::channel();
        tx.send(row("A")).unwrap();
        tx.send(row("B")).unwrap();
        drop(tx);
        app.drain(&rx);
        assert_eq!(app.cursor, 1); // pinned to last row
    }

    #[test]
    fn test_manual_up_navigation_pauses_follow() {
        let mut app = App::new(100);
        let (tx, rx) = mpsc::channel();
        tx.send(row("A")).unwrap();
        tx.send(row("B")).unwrap();
        drop(tx);
        app.drain(&rx);

        app.move_cursor_manual(-1);
        assert!(!app.following);

        let (tx2, rx2) = mpsc::channel();
        tx2.send(row("C")).unwrap();
        drop(tx2);
        app.drain(&rx2);
        assert_eq!(app.cursor, 0); // did not jump to the new bottom
    }

    #[test]
    fn test_resume_following_pins_bottom() {
        let mut app = App::new(100);
        let (tx, rx) = mpsc::channel();
        tx.send(row("A")).unwrap();
        tx.send(row("B")).unwrap();
        drop(tx);
        app.drain(&rx);
        app.move_cursor_manual(-1);
        assert!(!app.following);
        app.resume_following();
        assert!(app.following);
        assert_eq!(app.cursor, 1);
    }

    #[test]
    fn test_manual_down_also_pauses_follow() {
        let mut app = App::new(100);
        let (tx, rx) = mpsc::channel();
        tx.send(row("A")).unwrap();
        tx.send(row("B")).unwrap();
        drop(tx);
        app.drain(&rx);
        assert!(app.following);
        app.move_cursor_manual(0); // still counts as manual
        // delta 0 doesn't move but we always clear following in move_cursor_manual
        app.following = true;
        app.move_cursor_manual(1);
        assert!(!app.following);
    }

    #[test]
    fn test_rebuild_visible_follows_when_following_and_visible_set_grows() {
        let mut app = App::new(100);
        app.groups = GroupList {
            groups: vec![Group {
                label: "a".into(),
                chips: Vec::new(),
                expr: Some(Expr::parse("tag~A", false).unwrap()),
                time: None,
                enabled: true,
            }],
        };
        let (tx, rx) = mpsc::channel();
        tx.send(row("A")).unwrap();
        tx.send(row("B")).unwrap(); // doesn't match "a" group, filtered out
        drop(tx);
        app.drain(&rx);
        assert_eq!(app.visible, vec![0]); // only A is visible
        assert!(app.following);

        app.groups.groups.clear(); // simulates deleting the last filter group -> empty GroupList matches everything
        app.rebuild_visible();
        assert_eq!(app.visible, vec![0, 1]); // both now visible, set grew
        assert_eq!(app.cursor, 1); // still following: cursor pinned to new bottom (B), not stuck at old position
    }
}

#[cfg(test)]
mod search_tests {
    use super::*;
    use crate::search_model::SearchGroup;
    use std::sync::mpsc;

    fn row_with_msg(tag: &str, msg: &str) -> EntryRow {
        EntryRow::from_line(&format!("04-02 10:00:00.000  1234  5678 I {tag}   : {msg}")).unwrap()
    }

    #[test]
    fn test_find_match_next_prev_and_wrap() {
        let mut app = App::new(100);
        let (tx, rx) = mpsc::channel();
        tx.send(row_with_msg("T", "aaa")).unwrap();
        tx.send(row_with_msg("T", "hit one")).unwrap();
        tx.send(row_with_msg("T", "bbb")).unwrap();
        tx.send(row_with_msg("T", "hit two")).unwrap();
        drop(tx);
        app.drain(&rx);
        app.following = false;
        app.cursor = 0;
        app.search_groups
            .groups
            .push(SearchGroup::from_pattern("hit").unwrap());

        assert!(app.find_match(1));
        assert_eq!(app.cursor, 1);
        assert!(app.find_match(1));
        assert_eq!(app.cursor, 3);
        assert!(app.find_match(1)); // wrap
        assert_eq!(app.cursor, 1);
        assert!(app.find_match(-1)); // wrap backward to last
        assert_eq!(app.cursor, 3);
    }

    #[test]
    fn test_find_match_noop_without_search() {
        let mut app = App::new(100);
        let (tx, rx) = mpsc::channel();
        tx.send(row_with_msg("T", "x")).unwrap();
        drop(tx);
        app.drain(&rx);
        assert!(!app.find_match(1));
    }

    #[test]
    fn test_jump_first_match_and_search_match_stats() {
        let mut app = App::new(100);
        let (tx, rx) = mpsc::channel();
        tx.send(row_with_msg("T", "aaa")).unwrap();
        tx.send(row_with_msg("T", "hit one")).unwrap();
        tx.send(row_with_msg("T", "bbb")).unwrap();
        tx.send(row_with_msg("T", "hit two")).unwrap();
        drop(tx);
        app.drain(&rx);
        app.following = true;
        app.cursor = 3;
        assert!(app.search_match_stats().is_none());

        app.search_groups
            .groups
            .push(SearchGroup::from_pattern("hit").unwrap());
        app.cursor = 0; // non-hit row
        assert_eq!(app.search_match_stats(), Some((None, 2)));

        assert!(app.jump_first_match());
        assert_eq!(app.cursor, 1);
        assert!(!app.following);
        assert_eq!(app.search_match_stats(), Some((Some(1), 2)));

        app.cursor = 3;
        assert_eq!(app.search_match_stats(), Some((Some(2), 2)));

        app.cursor = 2;
        assert_eq!(app.search_match_stats(), Some((None, 2)));
    }

    #[test]
    fn test_jump_first_match_noop_when_no_hits() {
        let mut app = App::new(100);
        let (tx, rx) = mpsc::channel();
        tx.send(row_with_msg("T", "aaa")).unwrap();
        drop(tx);
        app.drain(&rx);
        app.cursor = 0;
        app.search_groups
            .groups
            .push(SearchGroup::from_pattern("zzz").unwrap());
        assert!(!app.jump_first_match());
        assert_eq!(app.cursor, 0);
        assert_eq!(app.search_match_stats(), Some((None, 0)));
    }

    #[test]
    fn test_jump_first_match_targets_newest_group_only() {
        let mut app = App::new(100);
        let (tx, rx) = mpsc::channel();
        tx.send(row_with_msg("T", "foo early")).unwrap();
        tx.send(row_with_msg("T", "bar later")).unwrap();
        tx.send(row_with_msg("T", "foo late")).unwrap();
        drop(tx);
        app.drain(&rx);
        app.following = false;
        app.cursor = 0;

        app.search_groups
            .groups
            .push(SearchGroup::from_pattern("foo").unwrap());
        app.search_groups
            .groups
            .push(SearchGroup::from_pattern("bar").unwrap());

        assert!(app.jump_first_match());
        // Must land on newest group ("bar"), not the earlier "foo" at index 0.
        assert_eq!(app.cursor, 1);
        assert_eq!(app.rows[app.visible[app.cursor]].msg, "bar later");
    }

    #[test]
    fn test_find_match_ignore_case_by_default() {
        let mut app = App::new(100);
        let (tx, rx) = mpsc::channel();
        tx.send(row_with_msg("T", "an error occurred")).unwrap();
        drop(tx);
        app.drain(&rx);
        app.following = false;
        app.cursor = 0;
        app.search_groups
            .groups
            .push(SearchGroup::from_pattern("ERROR").unwrap());
        assert!(app.search_groups.any_match("an error occurred"));
    }

    #[test]
    fn test_disabled_search_group_excluded_from_find() {
        let mut app = App::new(100);
        let (tx, rx) = mpsc::channel();
        tx.send(row_with_msg("T", "hit")).unwrap();
        drop(tx);
        app.drain(&rx);
        let mut g = SearchGroup::from_pattern("hit").unwrap();
        g.enabled = false;
        app.search_groups.groups.push(g);
        assert!(!app.find_match(1));
    }

    #[test]
    fn test_push_or_find_search_group_dedups() {
        let mut app = App::new(100);
        let idx0 = app.push_or_find_search_group(SearchGroup::from_pattern("foo").unwrap());
        assert_eq!(idx0, 0);
        let idx1 = app.push_or_find_search_group(SearchGroup::from_pattern("FOO").unwrap());
        assert_eq!(idx1, 0);
        assert_eq!(app.search_groups.groups.len(), 1);
    }
}

#[cfg(test)]
mod yank_and_search_tests {
    use super::*;
    use std::sync::mpsc;

    fn row_with_msg(tag: &str, msg: &str) -> EntryRow {
        EntryRow::from_line(&format!("04-02 10:00:00.000  1234  5678 I {tag}   : {msg}")).unwrap()
    }

    #[test]
    fn test_yank_field_extracts_tag_and_msg() {
        let mut app = App::new(100);
        let (tx, rx) = mpsc::channel();
        tx.send(row_with_msg("MyTag", "hello")).unwrap();
        drop(tx);
        app.drain(&rx);
        assert_eq!(app.yank_field(YankField::Tag).as_deref(), Some("MyTag"));
        assert_eq!(app.yank_field(YankField::Msg).as_deref(), Some("hello"));
        assert_eq!(app.yank_field(YankField::Pid).as_deref(), Some("1234"));
        assert_eq!(app.yank_field(YankField::Tid).as_deref(), Some("5678"));
        assert_eq!(app.yank_field(YankField::Level).as_deref(), Some("I"));
        assert_eq!(app.yank_field(YankField::Timestamp).as_deref(), Some("04-02 10:00:00.000"));
    }

    #[test]
    fn test_yank_range_joins_raw_with_newlines() {
        let mut app = App::new(100);
        let (tx, rx) = mpsc::channel();
        let a = row_with_msg("A", "one");
        let b = row_with_msg("B", "two");
        let raw_a = a.raw.clone();
        let raw_b = b.raw.clone();
        tx.send(a).unwrap();
        tx.send(b).unwrap();
        drop(tx);
        app.drain(&rx);
        let text = app.yank_range(0, 1, YankField::Raw).unwrap();
        assert_eq!(text, format!("{raw_a}\n{raw_b}"));
    }

    #[test]
    fn test_selection_range_orders_anchor_and_cursor() {
        let mut app = App::new(100);
        let (tx, rx) = mpsc::channel();
        for i in 0..5 {
            tx.send(row_with_msg("T", &format!("m{i}"))).unwrap();
        }
        drop(tx);
        app.drain(&rx);
        app.following = false;
        app.cursor = 3;
        app.visual_anchor = Some(1);
        assert_eq!(app.selection_range(), Some((1, 3)));
        app.cursor = 0;
        assert_eq!(app.selection_range(), Some((0, 1)));
    }

    #[test]
    fn test_yank_field_from_char_mapping() {
        assert_eq!(YankField::from_char('y'), Some(YankField::Raw));
        assert_eq!(YankField::from_char('t'), Some(YankField::Tag));
        assert_eq!(YankField::from_char('m'), Some(YankField::Msg));
        assert_eq!(YankField::from_char('T'), Some(YankField::Tid));
        assert_eq!(YankField::from_char('x'), None);
    }
}
