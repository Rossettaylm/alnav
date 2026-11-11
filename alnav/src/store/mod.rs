//! Row storage backends: mmap file (`FileStore`) and live stream (`StreamStore`).
//!
//! Readers go through [`RowStore::row_at_source`] / [`App::row_at`](crate::app::App::row_at)
//! returning [`RowRef`] (borrowed for stream, owned lazy-parse for file).

use std::ops::Deref;

use crate::model::EntryRow;

mod file;
mod stream;
#[cfg(test)]
mod tests;
mod workers;

pub use self::file::{FileEvent, FileStore, FilterPred};
pub use self::stream::StreamStore;

/// Byte range of one line inside the mmap (excluding `\n`; `\r` stripped).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineSpan {
    pub start: u64,
    pub len: u32,
}

/// Borrowed (stream) or owned (file lazy-parse) row handle.
#[derive(Debug)]
pub enum RowRef<'a> {
    Borrowed(&'a EntryRow),
    Owned(EntryRow),
}

impl Deref for RowRef<'_> {
    type Target = EntryRow;

    fn deref(&self) -> &EntryRow {
        match self {
            RowRef::Borrowed(r) => r,
            RowRef::Owned(r) => r,
        }
    }
}

impl RowRef<'_> {
    pub fn into_owned(self) -> EntryRow {
        match self {
            RowRef::Borrowed(r) => r.clone(),
            RowRef::Owned(r) => r,
        }
    }
}

/// Unified row backend.
pub enum RowStore {
    File(FileStore),
    Stream(StreamStore),
}

impl RowStore {
    pub fn stream(max_lines: usize, matched_cap: usize) -> Self {
        RowStore::Stream(StreamStore::new(max_lines, matched_cap))
    }

    pub fn is_file(&self) -> bool {
        matches!(self, RowStore::File(_))
    }

    pub fn as_stream(&self) -> Option<&StreamStore> {
        match self {
            RowStore::Stream(s) => Some(s),
            RowStore::File(_) => None,
        }
    }

    pub fn as_stream_mut(&mut self) -> Option<&mut StreamStore> {
        match self {
            RowStore::Stream(s) => Some(s),
            RowStore::File(_) => None,
        }
    }

    pub fn as_file(&self) -> Option<&FileStore> {
        match self {
            RowStore::File(f) => Some(f),
            RowStore::Stream(_) => None,
        }
    }

    pub fn as_file_mut(&mut self) -> Option<&mut FileStore> {
        match self {
            RowStore::File(f) => Some(f),
            RowStore::Stream(_) => None,
        }
    }

    pub fn source_len(&self, filter_active: bool) -> usize {
        match self {
            RowStore::Stream(s) => s.view_source(filter_active).len(),
            RowStore::File(f) => f.line_count(),
        }
    }

    pub fn row_at_source(&self, source_idx: usize, filter_active: bool) -> Option<RowRef<'_>> {
        match self {
            RowStore::Stream(s) => s
                .view_source(filter_active)
                .get(source_idx)
                .map(RowRef::Borrowed),
            RowStore::File(f) => f.row_at(source_idx).map(RowRef::Owned),
        }
    }

    /// Find source index of `row_id` in the active view (stream) or by
    /// `row_id - 1` line mapping (file).
    pub fn find_row_id(&self, row_id: u64, filter_active: bool) -> Option<usize> {
        match self {
            RowStore::Stream(s) => s
                .view_source(filter_active)
                .iter()
                .position(|r| r.row_id == row_id),
            RowStore::File(f) => {
                if row_id == 0 {
                    return None;
                }
                let i = (row_id - 1) as usize;
                if i < f.line_count() {
                    Some(i)
                } else {
                    None
                }
            }
        }
    }

    pub fn row_alive(&self, row_id: u64) -> bool {
        match self {
            RowStore::Stream(s) => {
                s.matched.iter().any(|r| r.row_id == row_id)
                    || s.rows.iter().any(|r| r.row_id == row_id)
            }
            RowStore::File(f) => {
                if row_id == 0 {
                    return false;
                }
                ((row_id - 1) as usize) < f.line_count()
            }
        }
    }
}
