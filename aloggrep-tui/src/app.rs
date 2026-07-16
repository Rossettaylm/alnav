use std::collections::VecDeque;
use std::sync::mpsc::Receiver;

use regex::Regex;

use crate::filter_model::GroupList;
use crate::model::EntryRow;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    ChipStrip,
    LogList,
    Input,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Insert,
}

pub struct App {
    pub rows: VecDeque<EntryRow>,
    pub visible: Vec<usize>,
    pub groups: GroupList,
    pub cursor: usize,
    pub max_lines: usize,
    pub should_quit: bool,
    pub focus: Focus,
    pub mode: Mode,
    pub group_cursor: usize,
    pub pending_dd: bool,
    pub following: bool,
    pub highlight: Option<Regex>,
    pub search_draft: Option<String>,
}

impl App {
    pub fn new(max_lines: usize) -> Self {
        Self {
            rows: VecDeque::new(),
            visible: Vec::new(),
            groups: GroupList::default(),
            cursor: 0,
            max_lines,
            should_quit: false,
            focus: Focus::LogList,
            mode: Mode::Normal,
            group_cursor: 0,
            pending_dd: false,
            following: true,
            highlight: None,
            search_draft: None,
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
            }
            for i in self.visible.iter_mut() {
                *i -= 1;
            }
            if evicted_was_visible && self.cursor > 0 {
                self.cursor -= 1;
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

    /// Manual upward movement pauses following; jumping to bottom resumes it.
    pub fn move_cursor_manual(&mut self, delta: isize) {
        if delta < 0 {
            self.following = false;
        }
        self.move_cursor(delta);
    }

    pub fn jump_bottom_resume_follow(&mut self) {
        self.following = true;
        self.jump_bottom();
    }

    pub fn visible_rows(&self) -> impl Iterator<Item = &EntryRow> {
        self.visible.iter().map(move |&i| &self.rows[i])
    }

    pub fn cycle_focus_forward(&mut self) {
        self.focus = match self.focus {
            Focus::ChipStrip => Focus::LogList,
            Focus::LogList => Focus::Input,
            Focus::Input => Focus::ChipStrip,
        };
    }

    pub fn cycle_focus_backward(&mut self) {
        self.focus = match self.focus {
            Focus::ChipStrip => Focus::Input,
            Focus::LogList => Focus::ChipStrip,
            Focus::Input => Focus::LogList,
        };
    }

    pub fn move_group_cursor(&mut self, delta: isize) {
        let len = self.groups.groups.len();
        if len == 0 {
            return;
        }
        let new = self.group_cursor as isize + delta;
        self.group_cursor = new.clamp(0, len as isize - 1) as usize;
    }

    /// First `d` arms `pending_dd`; a second `d` within the same keypress
    /// dispatch deletes the focused group and re-filters. Any other key
    /// clears `pending_dd` (handled by the caller in Task 14's key dispatch).
    pub fn delete_focused_group(&mut self) {
        if self.groups.groups.is_empty() {
            return;
        }
        self.groups.groups.remove(self.group_cursor);
        if self.group_cursor >= self.groups.groups.len() {
            self.group_cursor = self.groups.groups.len().saturating_sub(1);
        }
        self.rebuild_visible();
    }

    /// Independent of the chip filter system: never hides rows, only marks
    /// which ones should be highlighted when rendered.
    pub fn set_highlight(&mut self, pattern: &str) -> Result<(), String> {
        if pattern.is_empty() {
            self.highlight = None;
            return Ok(());
        }
        self.highlight = Some(Regex::new(pattern).map_err(|e| e.to_string())?);
        Ok(())
    }

    pub fn clear_highlight(&mut self) {
        self.highlight = None;
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
    fn test_cursor_unaffected_when_evicted_row_was_already_filtered_out() {
        let mut app = App::new(3);
        app.groups = GroupList {
            groups: vec![Group {
                label: "x".into(),
                expr: Some(Expr::parse("tag~X", false).unwrap()),
                time: None,
            }],
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

    #[test]
    fn test_cycle_focus_forward_wraps() {
        let mut app = App::new(100);
        assert_eq!(app.focus, Focus::LogList);
        app.cycle_focus_forward();
        assert_eq!(app.focus, Focus::Input);
        app.cycle_focus_forward();
        assert_eq!(app.focus, Focus::ChipStrip);
        app.cycle_focus_forward();
        assert_eq!(app.focus, Focus::LogList);
    }

    #[test]
    fn test_delete_focused_group_removes_and_rescans() {
        let mut app = App::new(100);
        app.groups.groups.push(Group { label: "g0".into(), expr: None, time: None });
        app.groups.groups.push(Group { label: "g1".into(), expr: None, time: None });
        app.group_cursor = 0;
        app.delete_focused_group();
        assert_eq!(app.groups.groups.len(), 1);
        assert_eq!(app.groups.groups[0].label, "g1");
    }

    #[test]
    fn test_move_group_cursor_clamps() {
        let mut app = App::new(100);
        app.groups.groups.push(Group { label: "g0".into(), expr: None, time: None });
        app.move_group_cursor(-5);
        assert_eq!(app.group_cursor, 0);
        app.move_group_cursor(5);
        assert_eq!(app.group_cursor, 0);
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
    fn test_jump_bottom_resumes_follow() {
        let mut app = App::new(100);
        let (tx, rx) = mpsc::channel();
        tx.send(row("A")).unwrap();
        drop(tx);
        app.drain(&rx);
        app.move_cursor_manual(-1);
        app.jump_bottom_resume_follow();
        assert!(app.following);
    }

    #[test]
    fn test_rebuild_visible_follows_when_following_and_visible_set_grows() {
        let mut app = App::new(100);
        app.groups = GroupList {
            groups: vec![
                Group { label: "a".into(), expr: Some(Expr::parse("tag~A", false).unwrap()), time: None },
            ],
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
mod highlight_tests {
    use super::*;

    #[test]
    fn test_set_highlight_compiles_regex() {
        let mut app = App::new(100);
        app.set_highlight("time.*out").unwrap();
        assert!(app.highlight.is_some());
    }

    #[test]
    fn test_set_highlight_empty_clears() {
        let mut app = App::new(100);
        app.set_highlight("x").unwrap();
        app.set_highlight("").unwrap();
        assert!(app.highlight.is_none());
    }

    #[test]
    fn test_set_highlight_bad_regex_errors() {
        let mut app = App::new(100);
        assert!(app.set_highlight("(unclosed").is_err());
    }
}
