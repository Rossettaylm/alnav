//! Filter predicates over rows, visible-set rebuilds, and `App` row lookups via [`LogIndex`].

use super::{App, ViewFocus, Visible};
use crate::highlight_model::HighlightGroupList;
use crate::model::EntryRow;
use crate::store::{RowRef, StreamStore};
use alnav::expr::Expr;
use std::cmp::Ordering;
use std::collections::{HashSet, VecDeque};
use std::sync::Arc;

fn row_time_order(a: &EntryRow, b: &EntryRow) -> Ordering {
    match (a.as_log_entry().time_full(), b.as_log_entry().time_full()) {
        (Some(ta), Some(tb)) => ta.cmp(tb).then_with(|| a.row_id.cmp(&b.row_id)),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => a.row_id.cmp(&b.row_id),
    }
}

impl App {
    /// Stream rolling buffer (tests / live). Empty for File.
    pub fn rows(&self) -> &VecDeque<EntryRow> {
        self.index.rows()
    }

    /// Stream matched buffer (tests / live). Empty for File.
    pub fn matched(&self) -> &VecDeque<EntryRow> {
        self.index.matched()
    }

    pub(super) fn stream_mut(&mut self) -> &mut StreamStore {
        self.index.stream_mut()
    }

    /// Visible slot → row (lazy-parse for File).
    pub fn row_at(&self, vis_i: usize) -> Option<RowRef<'_>> {
        self.index.row_at(vis_i, self.filter_active())
    }

    /// File-mode sessions may interactively edit the global time window.
    pub fn is_file_mode(&self) -> bool {
        self.export_source.is_file()
    }

    /// Compiled rule predicate → view focus. Used by drain/rebuild.
    ///
    /// CLI-aligned: when any filter is active, unparsed (raw-fallback) rows never pass.
    pub fn row_passes_filters(&self, row: &EntryRow) -> bool {
        debug_assert!(
            self.predicate.as_deref() == self.compiled_predicate().as_ref(),
            "rule state changed without rebuild_visible()"
        );
        Self::row_passes_filter_parts(
            row,
            self.predicate.as_deref(),
            self.view_focus,
            &self.highlight_groups,
            /*reject_unparsed*/ self.filter_active(),
        )
    }

    /// Shared predicate for stream `row_passes_filters` and file `FilterPred`.
    /// File scans only run while filters are active → pass `reject_unparsed = true`.
    pub(crate) fn row_passes_filter_parts(
        row: &EntryRow,
        predicate: Option<&Expr>,
        view_focus: ViewFocus,
        highlight_groups: &HighlightGroupList,
        reject_unparsed: bool,
    ) -> bool {
        if reject_unparsed && !row.is_parsed() {
            return false;
        }
        if predicate.is_some_and(|p| !p.matches(&row.as_log_entry())) {
            return false;
        }
        if view_focus.highlight && !highlight_groups.any_match_entry(row) {
            return false;
        }
        if view_focus.severe && !row.severe {
            return false;
        }
        true
    }

    /// The Filter / Exclude / lock / time-window rules as one expression.
    pub fn compiled_predicate(&self) -> Option<Expr> {
        crate::filter_model::compile_predicate(
            &self.groups,
            self.lock_pid.as_deref(),
            self.lock_tid.as_deref(),
            self.time_bound.as_ref(),
        )
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
    #[cfg(test)]
    pub fn view_source(&self) -> &VecDeque<EntryRow> {
        self.index.view_source(self.filter_active())
    }

    /// Source index for visible slot `vis_i`.
    pub fn source_idx_for_visible(&self, vis_i: usize) -> Option<usize> {
        self.index.visible.source_idx(vis_i)
    }

    /// Whether `row_id` is still present (bookmark liveness).
    pub(super) fn row_alive(&self, row_id: u64) -> bool {
        self.index.store.row_alive(row_id)
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
        self.index.visible = Visible::All {
            len: stream.matched.len(),
        };
    }

    pub(super) fn remove_matched_row_id(&mut self, row_id: u64) {
        let Some(idx) = self.matched().iter().position(|r| r.row_id == row_id) else {
            return;
        };
        self.stream_mut().matched.remove(idx);
        self.index.visible = Visible::All {
            len: self.matched().len(),
        };
        self.adjust_after_mid_visible_evict(idx);
    }

    /// Drop a pin from the visible set when it no longer passes rules.
    pub(super) fn unpin_from_visible(&mut self, row_id: u64) {
        if !self.filter_active() {
            return;
        }
        if self.index.store.is_file() {
            let Some(src) = self.index.store.find_row_id(row_id, false) else {
                return;
            };
            if self
                .row_by_id(row_id)
                .is_some_and(|row| self.row_passes_filters(&row))
            {
                return;
            }
            if let Visible::Subset(v) = &mut self.index.visible {
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
        // Every rule mutation ends here, so this is the one recompile point.
        self.predicate = self.compiled_predicate().map(Arc::new);
        let active = self.filter_active();
        if self.index.store.is_file() {
            if !active {
                if let Some(file) = self.index.store.as_file_mut() {
                    file.cancel_filter_scan();
                    let n = file.line_count();
                    let done = file.index_done();
                    self.index.filter_gen = 0;
                    self.index.visible = Visible::All { len: n };
                    self.ingest_done = done;
                }
            } else {
                self.index.visible = Visible::Subset(self.alive_bookmark_file_line_indices());
                self.ingest_done = false;
                let predicate = self.predicate.clone();
                let view_focus = self.view_focus;
                let highlight_groups = self.highlight_groups.clone();
                let pred: crate::store::FilterPred = Arc::new(move |row: &EntryRow| {
                    Self::row_passes_filter_parts(
                        row,
                        predicate.as_deref(),
                        view_focus,
                        &highlight_groups,
                        true,
                    )
                });
                let gen = self
                    .index
                    .store
                    .as_file_mut()
                    .expect("file")
                    .start_filter_scan(pred);
                self.index.filter_gen = gen;
            }
        } else if active {
            self.rebuild_stream_matched_with_pins();
        } else {
            let stream = self.stream_mut();
            stream.matched.clear();
            self.index.visible = Visible::All {
                len: stream.rows.len(),
            };
        }

        if self.following {
            self.jump_bottom();
        } else if self.cursor >= self.index.visible.len() {
            self.cursor = self.index.visible.len().saturating_sub(1);
        }
        self.match_stats_stale = true;
        // File: filter change invalidates Vis domain — restart highlight Inc.
        if self.index.store.is_file() {
            self.restart_highlight_scan();
        }
    }

    pub fn current_row(&self) -> Option<RowRef<'_>> {
        self.row_at(self.cursor)
    }

    /// Resolve a bookmarked `row_id` to an owned row (Stream: rows or matched;
    /// File: `row_id - 1`). Returns `None` when the row has left the buffers.
    pub fn row_by_id(&self, row_id: u64) -> Option<EntryRow> {
        self.index.row_by_id(row_id)
    }

    /// Map `row_id` → visible slot without parsing file lines.
    pub fn visible_idx_for_row_id(&self, row_id: u64) -> Option<usize> {
        self.index
            .visible_idx_for_row_id(row_id, self.filter_active())
    }
}
