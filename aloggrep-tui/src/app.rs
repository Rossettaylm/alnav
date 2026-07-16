use std::collections::VecDeque;
use std::sync::mpsc::Receiver;

use crate::filter_model::GroupList;
use crate::model::EntryRow;

pub struct App {
    pub rows: VecDeque<EntryRow>,
    pub visible: Vec<usize>,
    pub groups: GroupList,
    pub cursor: usize,
    pub max_lines: usize,
    pub should_quit: bool,
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
            // indices shift down by one; drop now-invalid 0 and shift the rest
            self.visible.retain(|&i| i > 0);
            for i in self.visible.iter_mut() {
                *i -= 1;
            }
            if self.cursor > 0 {
                self.cursor -= 1;
            }
        }
        let matches = self.groups.matches(&row);
        self.rows.push_back(row);
        if matches {
            self.visible.push(self.rows.len() - 1);
        }
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
        if self.cursor >= self.visible.len() {
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

    pub fn visible_rows(&self) -> impl Iterator<Item = &EntryRow> {
        self.visible.iter().map(move |&i| &self.rows[i])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
}
