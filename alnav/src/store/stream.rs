use std::collections::VecDeque;

use crate::model::EntryRow;

/// Live / channel ingest buffer (`--hdc`, `--adb`, and tests).
#[derive(Debug)]
pub struct StreamStore {
    pub rows: VecDeque<EntryRow>,
    pub matched: VecDeque<EntryRow>,
    pub max_lines: usize,
    pub matched_cap: usize,
}

impl StreamStore {
    pub fn new(max_lines: usize, matched_cap: usize) -> Self {
        Self {
            rows: VecDeque::new(),
            matched: VecDeque::new(),
            max_lines,
            matched_cap,
        }
    }

    pub fn clear(&mut self) {
        self.rows.clear();
        self.matched.clear();
    }

    pub fn view_source(&self, filter_active: bool) -> &VecDeque<EntryRow> {
        if filter_active {
            &self.matched
        } else {
            &self.rows
        }
    }
}
