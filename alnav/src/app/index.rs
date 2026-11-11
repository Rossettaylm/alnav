//! Message index: row backend (`RowStore`) plus the visible-slot mapping into it.
//!
//! Views (log list, minimap, preview, summary/hist jobs) are projections over
//! this index; filter predicates live on `App` because they read rule state.

/// Visible-row index set into the active row source.
///
/// Stream path (`--hdc` / `--adb`) always uses [`Visible::All`]: an identity mapping
/// `0..len` — never a materialised `Vec<usize>`. File path uses [`Visible::All`]
/// when unfiltered and [`Visible::Subset`] (hit line numbers) when filtered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Visible {
    /// Identity: visible slot `i` maps to source index `i`.
    All { len: usize },
    /// Sparse hits (file/mmap filter): visible slot `i` → `hits[i]` line index.
    Subset(Vec<usize>),
}

impl Visible {
    pub fn len(&self) -> usize {
        match self {
            Visible::All { len } => *len,
            Visible::Subset(v) => v.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn clear(&mut self) {
        *self = Visible::All { len: 0 };
    }

    /// Source index for visible slot `vis_i`.
    pub fn source_idx(&self, vis_i: usize) -> Option<usize> {
        match self {
            Visible::All { len } if vis_i < *len => Some(vis_i),
            Visible::All { .. } => None,
            Visible::Subset(v) => v.get(vis_i).copied(),
        }
    }
}

impl Default for Visible {
    fn default() -> Self {
        Visible::All { len: 0 }
    }
}

use crate::model::EntryRow;
use crate::store::{RowRef, RowStore, StreamStore};
use std::collections::VecDeque;

pub struct LogIndex {
    /// Stream (live/tests) or mmap file (`-f`) row backend.
    pub store: RowStore,
    /// Visible slot → source index into [`Self::store`].
    pub visible: Visible,
    /// Hard cap on stream `matched` (OOM safety); tests override. Ignored for File.
    pub matched_cap: usize,
    /// Active File filter-scan generation (stale batches ignored).
    pub(super) filter_gen: u64,
}

impl LogIndex {
    pub fn new(max_lines: usize, matched_cap: usize) -> Self {
        Self {
            store: RowStore::stream(max_lines, matched_cap),
            visible: Visible::default(),
            matched_cap,
            filter_gen: 0,
        }
    }

    /// Drop all rows and return to an empty stream backend (source switch).
    pub fn reset_stream(&mut self, max_lines: usize) {
        self.store = RowStore::stream(max_lines, self.matched_cap);
        self.visible = Visible::default();
        self.filter_gen = 0;
    }

    pub fn is_file(&self) -> bool {
        self.store.is_file()
    }

    /// Stream rolling buffer. Empty for File.
    pub fn rows(&self) -> &VecDeque<EntryRow> {
        match &self.store {
            RowStore::Stream(s) => &s.rows,
            RowStore::File(_) => empty_rows(),
        }
    }

    /// Stream matched buffer. Empty for File.
    pub fn matched(&self) -> &VecDeque<EntryRow> {
        match &self.store {
            RowStore::Stream(s) => &s.matched,
            RowStore::File(_) => empty_rows(),
        }
    }

    pub fn stream_mut(&mut self) -> &mut StreamStore {
        self.store
            .as_stream_mut()
            .expect("stream operation on file store")
    }

    /// Stream-only: the buffer `visible` indexes (`matched` when filtered). Empty for File.
    #[cfg(test)]
    pub fn view_source(&self, filter_active: bool) -> &VecDeque<EntryRow> {
        match &self.store {
            RowStore::Stream(s) => s.view_source(filter_active),
            RowStore::File(_) => empty_rows(),
        }
    }

    /// Visible slot → row (lazy-parse for File). `filter_active` selects the
    /// Stream buffer (`matched` vs `rows`); File ignores it.
    pub fn row_at(&self, vis_i: usize, filter_active: bool) -> Option<RowRef<'_>> {
        let src = self.visible.source_idx(vis_i)?;
        let stream_filtered = !self.is_file() && filter_active;
        self.store.row_at_source(src, stream_filtered)
    }

    /// Resolve `row_id` to an owned row (Stream: `matched` then `rows`;
    /// File: line `row_id - 1`). `None` once the row has left the buffers.
    pub fn row_by_id(&self, row_id: u64) -> Option<EntryRow> {
        match &self.store {
            RowStore::Stream(s) => s
                .matched
                .iter()
                .chain(s.rows.iter())
                .find(|r| r.row_id == row_id)
                .cloned(),
            RowStore::File(f) => {
                if row_id == 0 {
                    return None;
                }
                f.row_at((row_id - 1) as usize)
            }
        }
    }

    /// Map `row_id` → visible slot without parsing file lines.
    /// `Subset` hits are append-ordered, so lookup is binary search.
    pub fn visible_idx_for_row_id(&self, row_id: u64, filter_active: bool) -> Option<usize> {
        let stream_filtered = !self.is_file() && filter_active;
        let src_idx = self.store.find_row_id(row_id, stream_filtered)?;
        match &self.visible {
            Visible::All { len } if src_idx < *len => Some(src_idx),
            Visible::All { .. } => None,
            Visible::Subset(v) => v.binary_search(&src_idx).ok(),
        }
    }
}

fn empty_rows() -> &'static VecDeque<EntryRow> {
    static EMPTY: VecDeque<EntryRow> = VecDeque::new();
    &EMPTY
}
