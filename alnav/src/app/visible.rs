//! Visible-row index (`Visible`), filter predicates, and row lookup over the active store.

use super::{App, ViewFocus};
use crate::highlight_model::HighlightGroupList;
use crate::model::EntryRow;
use crate::store::{RowRef, RowStore, StreamStore};
use std::cmp::Ordering;
use std::collections::{HashSet, VecDeque};
use std::sync::Arc;

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

fn row_time_order(a: &EntryRow, b: &EntryRow) -> Ordering {
    match (a.as_log_entry().time_full(), b.as_log_entry().time_full()) {
        (Some(ta), Some(tb)) => ta.cmp(tb).then_with(|| a.row_id.cmp(&b.row_id)),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => a.row_id.cmp(&b.row_id),
    }
}

impl App {
    /// Stream rolling buffer (tests / live). Empty static for File.
    pub fn rows(&self) -> &VecDeque<EntryRow> {
        static EMPTY: std::sync::OnceLock<VecDeque<EntryRow>> = std::sync::OnceLock::new();
        match &self.store {
            RowStore::Stream(s) => &s.rows,
            RowStore::File(_) => EMPTY.get_or_init(VecDeque::new),
        }
    }

    /// Stream matched buffer (tests / live). Empty static for File.
    pub fn matched(&self) -> &VecDeque<EntryRow> {
        static EMPTY: std::sync::OnceLock<VecDeque<EntryRow>> = std::sync::OnceLock::new();
        match &self.store {
            RowStore::Stream(s) => &s.matched,
            RowStore::File(_) => EMPTY.get_or_init(VecDeque::new),
        }
    }

    pub(super) fn stream_mut(&mut self) -> &mut StreamStore {
        self.store
            .as_stream_mut()
            .expect("stream operation on file store")
    }

    /// Visible slot → row (lazy-parse for File).
    pub fn row_at(&self, vis_i: usize) -> Option<RowRef<'_>> {
        let src = self.source_idx_for_visible(vis_i)?;
        // File ignores filter_active for buffer selection (source is line idx).
        let filter_active = matches!(self.store, RowStore::Stream(_)) && self.filter_active();
        self.store.row_at_source(src, filter_active)
    }

    /// Whether the session still has no bound source (Dashboard active).
    pub fn is_unbound(&self) -> bool {
        self.dashboard.is_some()
    }

    /// File-mode sessions may interactively edit the global time window.
    pub fn is_file_mode(&self) -> bool {
        self.export_source.is_file()
    }

    /// Chip filter → session lock (H8) → global time → view focus. Used by drain/rebuild.
    ///
    /// CLI-aligned: when any filter is active, unparsed (raw-fallback) rows never pass.
    pub fn row_passes_filters(&self, row: &EntryRow) -> bool {
        Self::row_passes_filter_parts(
            row,
            &self.groups,
            self.lock_pid.as_deref(),
            self.lock_tid.as_deref(),
            self.time_bound.as_ref(),
            self.view_focus,
            &self.highlight_groups,
            /*reject_unparsed*/ self.filter_active(),
        )
    }

    /// Shared predicate for stream `row_passes_filters` and file `FilterPred`.
    /// File scans only run while filters are active → pass `reject_unparsed = true`.
    pub(crate) fn row_passes_filter_parts(
        row: &EntryRow,
        groups: &crate::filter_model::GroupList,
        lock_pid: Option<&str>,
        lock_tid: Option<&str>,
        time_bound: Option<&crate::filter_model::TimeBound>,
        view_focus: ViewFocus,
        highlight_groups: &HighlightGroupList,
        reject_unparsed: bool,
    ) -> bool {
        if reject_unparsed && !row.is_parsed() {
            return false;
        }
        if !groups.matches(row) {
            return false;
        }
        if let Some(pid) = lock_pid {
            if row.pid != *pid {
                return false;
            }
        } else if let Some(tid) = lock_tid {
            if row.tid != *tid {
                return false;
            }
        }
        if let Some(bound) = time_bound {
            if bound.is_active() && !bound.matches(&row.as_log_entry()) {
                return false;
            }
        }
        if view_focus.highlight && !highlight_groups.any_match_entry(row) {
            return false;
        }
        if view_focus.severe && !row.severe {
            return false;
        }
        true
    }

    /// Whether any include/exclude/lock/time/view-focus filter is currently active.
    /// When false, `visible` indexes `rows` directly (every row shown); when true,
    /// `visible` indexes `matched` (only filter-passing rows, retained across `rows` churn).
    pub fn filter_active(&self) -> bool {
        self.groups.has_any_enabled()
            || self.groups.excludes.iter().any(|e| e.enabled)
            || self.lock_pid.is_some()
            || self.lock_tid.is_some()
            || self.time_bound.as_ref().is_some_and(|t| t.is_active())
            || self.view_focus.is_active()
    }

    /// Stream-only: the buffer `visible` indexes (`matched` when filter
    /// active). Prefer [`Self::row_at`] for render paths (works for File too).
    pub fn view_source(&self) -> &VecDeque<EntryRow> {
        match &self.store {
            RowStore::Stream(s) => s.view_source(self.filter_active()),
            RowStore::File(_) => self.rows(), // empty
        }
    }

    /// Number of currently visible rows.
    pub fn visible_len(&self) -> usize {
        self.visible.len()
    }

    /// Source index for visible slot `vis_i`.
    pub fn source_idx_for_visible(&self, vis_i: usize) -> Option<usize> {
        self.visible.source_idx(vis_i)
    }

    /// Whether `row_id` is still present (bookmark liveness).
    pub(super) fn row_alive(&self, row_id: u64) -> bool {
        self.store.row_alive(row_id)
    }

    pub(super) fn alive_bookmark_file_line_indices(&self) -> Vec<usize> {
        let mut idxs: Vec<usize> = self
            .bookmarks
            .items
            .iter()
            .filter_map(|bm| {
                let id = bm.row_id();
                if !self.row_alive(id) || id == 0 {
                    return None;
                }
                Some((id - 1) as usize)
            })
            .collect();
        idxs.sort_unstable();
        idxs.dedup();
        idxs
    }

    fn rebuild_stream_matched_with_pins(&mut self) {
        let mut passing: Vec<EntryRow> = self
            .rows()
            .iter()
            .filter(|r| self.row_passes_filters(r))
            .cloned()
            .collect();
        let have: HashSet<u64> = passing.iter().map(|r| r.row_id).collect();
        let pin_rows: Vec<EntryRow> = self
            .bookmarks
            .items
            .iter()
            .filter_map(|bm| {
                let id = bm.row_id();
                if have.contains(&id) {
                    return None;
                }
                self.rows().iter().find(|r| r.row_id == id).cloned()
            })
            .collect();
        passing.extend(pin_rows);
        passing.sort_by(row_time_order);
        let stream = self.stream_mut();
        stream.matched.clear();
        stream.matched.extend(passing);
        self.visible = Visible::All {
            len: stream.matched.len(),
        };
    }

    pub(super) fn remove_matched_row_id(&mut self, row_id: u64) {
        let Some(idx) = self.matched().iter().position(|r| r.row_id == row_id) else {
            return;
        };
        self.stream_mut().matched.remove(idx);
        self.visible = Visible::All {
            len: self.matched().len(),
        };
        self.adjust_after_mid_visible_evict(idx);
    }

    /// Drop a pin from the visible set when it no longer passes rules.
    pub(super) fn unpin_from_visible(&mut self, row_id: u64) {
        if !self.filter_active() {
            return;
        }
        if self.store.is_file() {
            let Some(src) = self.store.find_row_id(row_id, false) else {
                return;
            };
            if self
                .row_by_id(row_id)
                .is_some_and(|row| self.row_passes_filters(&row))
            {
                return;
            }
            if let Visible::Subset(v) = &mut self.visible {
                if let Ok(i) = v.binary_search(&src) {
                    v.remove(i);
                    self.adjust_after_mid_visible_evict(i);
                }
            }
        } else {
            let Some(row) = self.matched().iter().find(|r| r.row_id == row_id).cloned() else {
                return;
            };
            if self.row_passes_filters(&row) {
                return;
            }
            self.remove_matched_row_id(row_id);
        }
        self.match_stats_stale = true;
    }

    /// Full rescan, used when the filter groups or session lock change.
    /// Stream: rebuilds `matched` from `rows`. File: starts a cancellable
    /// background filter scan into [`Visible::Subset`] (or All when inactive).
    pub fn rebuild_visible(&mut self) {
        let active = self.filter_active();
        if self.store.is_file() {
            if !active {
                if let Some(file) = self.store.as_file_mut() {
                    file.cancel_filter_scan();
                    let n = file.line_count();
                    let done = file.index_done();
                    self.file_filter_gen = 0;
                    self.visible = Visible::All { len: n };
                    self.ingest_done = done;
                }
            } else {
                self.visible = Visible::Subset(self.alive_bookmark_file_line_indices());
                self.ingest_done = false;
                let groups = self.groups.clone();
                let lock_pid = self.lock_pid.clone();
                let lock_tid = self.lock_tid.clone();
                let time_bound = self.time_bound.clone();
                let view_focus = self.view_focus;
                let highlight_groups = self.highlight_groups.clone();
                let pred: crate::store::FilterPred = Arc::new(move |row: &EntryRow| {
                    Self::row_passes_filter_parts(
                        row,
                        &groups,
                        lock_pid.as_deref(),
                        lock_tid.as_deref(),
                        time_bound.as_ref(),
                        view_focus,
                        &highlight_groups,
                        true,
                    )
                });
                let gen = self
                    .store
                    .as_file_mut()
                    .expect("file")
                    .start_filter_scan(pred);
                self.file_filter_gen = gen;
            }
        } else if active {
            self.rebuild_stream_matched_with_pins();
        } else {
            let stream = self.stream_mut();
            stream.matched.clear();
            self.visible = Visible::All {
                len: stream.rows.len(),
            };
        }

        if self.following {
            self.jump_bottom();
        } else if self.cursor >= self.visible.len() {
            self.cursor = self.visible.len().saturating_sub(1);
        }
        self.match_stats_stale = true;
        // File: filter change invalidates Vis domain — restart highlight Inc.
        if self.store.is_file() {
            self.restart_highlight_scan();
        }
    }

    /// Collect currently visible rows (owned clones; prefer [`Self::row_at`]
    /// for hot paths).
    pub fn visible_rows(&self) -> Vec<EntryRow> {
        let len = self.visible.len();
        (0..len)
            .filter_map(|i| self.row_at(i).map(|r| r.into_owned()))
            .collect()
    }

    pub fn current_row(&self) -> Option<RowRef<'_>> {
        self.row_at(self.cursor)
    }

    /// Resolve a bookmarked `row_id` to an owned row (Stream: rows or matched;
    /// File: `row_id - 1`). Returns `None` when the row has left the buffers.
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
                let i = (row_id - 1) as usize;
                f.row_at(i)
            }
        }
    }

    /// Map `row_id` → visible slot without parsing file lines.
    /// `Subset` hits are append-ordered, so lookup is binary search.
    pub fn visible_idx_for_row_id(&self, row_id: u64) -> Option<usize> {
        let filter_active = matches!(self.store, RowStore::Stream(_)) && self.filter_active();
        let src_idx = self.store.find_row_id(row_id, filter_active)?;
        match &self.visible {
            Visible::All { len } if src_idx < *len => Some(src_idx),
            Visible::All { .. } => None,
            Visible::Subset(v) => v.binary_search(&src_idx).ok(),
        }
    }
}
