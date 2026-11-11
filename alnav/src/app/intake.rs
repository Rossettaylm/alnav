//! Row intake: ingest drain, FileStore polling, push/evict bookkeeping, and highlight scan restarts.

use super::Pending;
use super::{App, Visible, DRAIN_BUDGET_PER_FRAME};
use crate::model::{is_severe_row, EntryRow};
use crate::scan::HighlightDomain;
use crate::store::{FileEvent, FileStore, RowStore};
use std::cmp::Ordering;
use std::sync::Arc;

/// Merge `incoming` into a sorted unique `dest` (File `Visible::Subset`).
fn merge_sorted_unique(dest: &mut Vec<usize>, incoming: &[usize]) {
    if incoming.is_empty() {
        return;
    }
    if dest.is_empty() {
        dest.extend_from_slice(incoming);
        dest.sort_unstable();
        dest.dedup();
        return;
    }
    let mut src = incoming.to_vec();
    src.sort_unstable();
    src.dedup();
    let mut out = Vec::with_capacity(dest.len() + src.len());
    let mut i = 0;
    let mut j = 0;
    while i < dest.len() && j < src.len() {
        match dest[i].cmp(&src[j]) {
            Ordering::Less => {
                out.push(dest[i]);
                i += 1;
            }
            Ordering::Greater => {
                out.push(src[j]);
                j += 1;
            }
            Ordering::Equal => {
                out.push(dest[i]);
                i += 1;
                j += 1;
            }
        }
    }
    out.extend_from_slice(&dest[i..]);
    out.extend_from_slice(&src[j..]);
    *dest = out;
}

impl App {
    /// Install a mmap file backend (replaces the default stream store).
    pub fn set_file_store(&mut self, file: FileStore) {
        let n = file.line_count();
        let done = file.index_done();
        self.index.store = RowStore::File(file);
        self.index.visible = Visible::All { len: n };
        self.ingest_done = done && !self.filter_active();
        self.index.filter_gen = 0;
        // Drain priming events (e.g. open_sync IndexDone) + start filter if needed.
        self.poll_file_store();
        if self.filter_active() {
            self.rebuild_visible();
        } else if self.following {
            self.jump_bottom();
        }
        self.restart_highlight_scan();
    }

    /// Drain pending rows from a stream ingest source without blocking.
    /// No-op for File (use [`Self::poll_file_store`]).
    pub fn drain(&mut self, ingest: &impl crate::ingest::TryRecvRow) {
        if self.index.store.is_file() {
            return;
        }
        let mut drained = 0usize;
        loop {
            if drained >= DRAIN_BUDGET_PER_FRAME {
                break;
            }
            match ingest.try_recv_row() {
                Ok(row) => {
                    self.push_row(row);
                    drained += 1;
                }
                Err(crate::ingest::TryRecvKind::Empty) => break,
                Err(crate::ingest::TryRecvKind::Disconnected) => {
                    self.ingest_done = true;
                    break;
                }
            }
        }
    }

    /// Poll mmap indexer / filter / highlight worker events (file mode). Call each frame.
    pub fn poll_file_store(&mut self) {
        let Some(_) = self.index.store.as_file() else {
            return;
        };
        let events = self
            .index
            .store
            .as_file()
            .map(|f| f.drain_events())
            .unwrap_or_default();
        let mut index_grew = false;
        let mut filter_batches: Vec<(u64, Vec<usize>)> = Vec::new();
        let mut filter_done_gen = None;
        let mut index_done = false;
        let mut hl_batches: Vec<(u64, Vec<usize>, usize)> = Vec::new();
        let mut hl_done_gen = None;
        for ev in events {
            match ev {
                FileEvent::IndexProgress { line_count, .. } => {
                    index_grew = true;
                    if let Some(f) = self.index.store.as_file() {
                        f.grow_severe_cache();
                    }
                    if !self.filter_active() {
                        self.index.visible = Visible::All { len: line_count };
                        if let Some(dom) = &self.highlight_domain {
                            dom.set_identity_len(line_count);
                        }
                    }
                }
                FileEvent::IndexDone { line_count } => {
                    index_grew = true;
                    index_done = true;
                    if let Some(f) = self.index.store.as_file() {
                        f.grow_severe_cache();
                    }
                    if !self.filter_active() {
                        self.index.visible = Visible::All { len: line_count };
                        if let Some(dom) = &self.highlight_domain {
                            dom.set_identity_len(line_count);
                            dom.seal();
                        }
                    }
                    // One-shot sampled vocab after index completes.
                    if let Some(f) = self.index.store.as_file() {
                        if f.mark_vocab_started() {
                            let mut feeds = Vec::new();
                            f.feed_vocab_sample(|tag, pkg, tokens| {
                                feeds.push((tag.to_string(), pkg.to_string(), tokens.to_vec()));
                            });
                            for (tag, pkg, tokens) in feeds {
                                self.vocab.feed(&tag, &pkg, &tokens);
                            }
                        }
                    }
                }
                FileEvent::FilterBatch { gen, hits, .. } => {
                    filter_batches.push((gen, hits));
                }
                FileEvent::FilterDone { gen, .. } => {
                    filter_done_gen = Some(gen);
                }
                FileEvent::HighlightBatch { gen, hits, scanned } => {
                    hl_batches.push((gen, hits, scanned));
                }
                FileEvent::HighlightDone { gen, scanned } => {
                    hl_done_gen = Some((gen, scanned));
                }
            }
        }
        let mut filter_grew = false;
        for (gen, hits) in filter_batches {
            if gen != self.index.filter_gen {
                continue;
            }
            // Inc: grow highlight domain with Subset so the worker can continue.
            if let Some(dom) = &self.highlight_domain {
                dom.extend_subset(&hits);
            }
            self.merge_file_filter_hits(&hits);
            filter_grew = true;
            // Never O(visible) parse here — highlight Inc follows Subset growth.
        }
        if filter_grew {
            self.match_stats_stale = true;
        }
        if let Some(gen) = filter_done_gen {
            if gen == self.index.filter_gen {
                // ensure Subset even if zero hits; re-merge pins after index growth
                if self.filter_active() {
                    self.merge_file_filter_hits(&[]);
                }
                if let Some(dom) = &self.highlight_domain {
                    dom.seal();
                }
                // Refresh cached ordinal from hit index (no full parse).
                self.match_stats_stale = true;
            }
        }
        for (gen, hits, scanned) in hl_batches {
            if gen != self.highlight_scan.gen {
                continue;
            }
            self.highlight_scan.hits.extend(hits);
            self.highlight_scan.scanned_vis = scanned;
            self.match_stats_stale = true;
            self.try_pending_jump_first();
        }
        if let Some((gen, scanned)) = hl_done_gen {
            if gen == self.highlight_scan.gen {
                self.highlight_scan.scanned_vis = scanned;
                self.highlight_scan.done = true;
                self.match_stats_stale = true;
                self.try_pending_jump_first();
            }
        }
        if index_grew || index_done || filter_grew {
            if self.following {
                self.jump_bottom();
            } else if self.cursor >= self.index.visible.len() {
                self.cursor = self.index.visible.len().saturating_sub(1);
            }
        }
        if index_done {
            // Cursor-only refresh from hit index — never full UI parse.
            self.match_stats_stale = true;
            if !self.filter_active() {
                self.ingest_done = true;
            }
        }
        if let Some(gen) = filter_done_gen {
            if gen == self.index.filter_gen && self.filter_active() {
                self.ingest_done = true;
            }
        }
        self.try_pending_restore_cursor();
    }

    /// Apply a restored cursor once its `row_id` resolves to a visible slot.
    ///
    /// `visible` can be non-empty while the line index still covers only a
    /// prefix of the file, so "first non-empty" is not a safe trigger: a
    /// mid-file `row_id` would resolve to `None` and the restore would be lost.
    /// Consume on success instead, and give up only once both scans have
    /// settled (the row is genuinely gone — file shrank or rotated, R8).
    ///
    /// `index_done` alone is not enough: a restored filter leaves `visible` an
    /// empty `Subset` until the (much slower) filter scan reports hits, and the
    /// index almost always finishes first. `ingest_done` is read instead of
    /// `FileStore::progress().filter_done` because it is set from the drained
    /// `FilterDone` event, which the FIFO channel guarantees arrives after every
    /// `FilterBatch` — the raw flag can flip while a final batch is still queued.
    fn try_pending_restore_cursor(&mut self) {
        let Some(row_id) = self.pending_restore_cursor else {
            return;
        };
        match self.visible_idx_for_row_id(row_id) {
            Some(idx) => {
                self.pending_restore_cursor = None;
                self.cursor = idx;
                self.list_offset = self.pending_restore_offset.min(self.cursor);
                self.pending_restore_offset = 0;
                self.match_stats_stale = true;
            }
            None if self.file_index_done() && self.ingest_done => {
                self.pending_restore_cursor = None;
                self.pending_restore_offset = 0;
                self.cursor = 0;
                self.list_offset = 0;
            }
            None => {}
        }
    }

    /// Whether the File line index finished. Always false for Stream.
    pub fn file_index_done(&self) -> bool {
        self.index.store.as_file().is_some_and(|f| f.index_done())
    }

    fn try_pending_jump_first(&mut self) {
        let Some(group_idx) = self.pending_jump_first else {
            return;
        };
        if self.active_highlight != Some(group_idx) {
            self.pending_jump_first = None;
            return;
        }
        if let Some(h) = self.highlight_scan.first_hit() {
            self.following = false;
            self.cursor = h;
            self.match_stats_stale = true;
            self.pending_jump_first = None;
        } else if self.highlight_scan.done {
            self.pending_jump_first = None;
        }
    }

    /// Status fragment for file load/filter progress (None when idle/stream).
    pub fn file_progress_label(&self) -> Option<String> {
        let f = self.index.store.as_file()?;
        let p = f.progress();
        if !p.index_done {
            // Approximate total lines from byte progress for status `idx a/b`.
            let approx_total = if p.file_bytes == 0 || p.indexed_bytes == 0 {
                p.indexed_lines.max(1)
            } else {
                let est = (p.indexed_lines as u64).saturating_mul(p.file_bytes as u64)
                    / (p.indexed_bytes as u64).max(1);
                (est as usize).max(p.indexed_lines).max(1)
            };
            return Some(format!("idx {}/{}", p.indexed_lines, approx_total));
        }
        if self.filter_active() && !p.filter_done {
            let total = p.indexed_lines.max(1);
            let pct = (p.filter_scanned.saturating_mul(100) / total).min(100);
            return Some(format!("FILTER {pct}%"));
        }
        None
    }

    /// LogList title/banner loading text (index / filter / highlight). Free: does
    /// not block input. Stream sessions always return `None`.
    pub fn log_loading_label(&self) -> Option<String> {
        let f = self.index.store.as_file()?;
        let p = f.progress();
        if !p.index_done {
            let pct = if p.file_bytes == 0 {
                100
            } else {
                (p.indexed_bytes.saturating_mul(100) / p.file_bytes).min(100)
            };
            return Some(format!("Indexing {pct}%…"));
        }
        if self.filter_active() && !p.filter_done {
            let total = p.indexed_lines.max(1);
            let pct = (p.filter_scanned.saturating_mul(100) / total).min(100);
            return Some(format!("Filtering {pct}%…"));
        }
        if self.active_highlight_group().is_some() && !self.highlight_scan.done {
            let total = self.index.visible.len().max(1);
            let pct = (self.highlight_scan.scanned_vis.saturating_mul(100) / total).min(100);
            return Some(format!("Highlight {pct}%…"));
        }
        None
    }

    /// Cancel + restart File highlight Vis scan for the active group (or clear).
    pub fn restart_highlight_scan(&mut self) {
        self.pending_jump_first = None;
        if !self.index.store.is_file() {
            self.highlight_scan.clear();
            self.highlight_domain = None;
            self.match_stats_stale = true;
            return;
        }
        let Some(group) = self.active_highlight_group().cloned() else {
            if let Some(f) = self.index.store.as_file_mut() {
                f.cancel_highlight_scan();
            }
            self.highlight_scan.clear();
            self.highlight_scan.done = true;
            self.highlight_domain = None;
            self.match_stats_stale = true;
            return;
        };
        let domain = match &self.index.visible {
            Visible::All { len } => {
                let d = HighlightDomain::identity(*len);
                // Seal when index already done and no filter growth expected.
                if let Some(f) = self.index.store.as_file() {
                    let p = f.progress();
                    if p.index_done && !self.filter_active() {
                        d.seal();
                    }
                }
                d
            }
            Visible::Subset(v) => {
                let d = HighlightDomain::subset(v.clone());
                if let Some(f) = self.index.store.as_file() {
                    if f.progress().filter_done {
                        d.seal();
                    }
                }
                d
            }
        };
        self.highlight_domain = Some(Arc::clone(&domain));
        self.highlight_scan.clear();
        let gen = self
            .index
            .store
            .as_file_mut()
            .expect("file")
            .start_highlight_scan(domain, &group.pattern);
        self.highlight_scan.gen = gen;
        self.highlight_scan.done = false;
        self.match_stats_stale = true;
    }

    pub(super) fn push_row(&mut self, mut row: EntryRow) {
        let msg_tokens = crate::input::tokenize_msg_for_vocab(&row.msg);
        self.vocab.feed(&row.tag, &row.pkg, &msg_tokens);
        row.row_id = self.next_row_id;
        self.next_row_id = self.next_row_id.wrapping_add(1);
        // P2: compute severe once at ingest so minimap/find_severe never re-run CrashDetector.
        row.severe = is_severe_row(&row);
        let active = self.filter_active();
        let matches = active && self.row_passes_filters(&row);
        let max_lines = self.max_lines;
        let matched_cap = self.index.matched_cap;

        let mut evict_rows = false;
        let mut evict_matched = false;
        let mut evicted_row: Option<EntryRow> = None;
        let mut mid_matched_evict: Option<usize> = None;
        let pin_ids = self.bookmark_row_ids.clone();
        {
            let stream = self.stream_mut();
            stream.max_lines = max_lines;
            stream.matched_cap = matched_cap;
            if stream.rows.len() >= stream.max_lines {
                evicted_row = stream.rows.pop_front();
                if evicted_row.is_some() && !active {
                    evict_rows = true;
                }
            }
            if matches && stream.matched.len() >= stream.matched_cap {
                if let Some(idx) = stream
                    .matched
                    .iter()
                    .position(|r| !pin_ids.contains(&r.row_id))
                {
                    stream.matched.remove(idx);
                    if idx == 0 {
                        evict_matched = true;
                    } else {
                        mid_matched_evict = Some(idx);
                    }
                }
            }
        }
        if evict_rows || evict_matched {
            self.adjust_after_identity_front_evict();
        }
        if let Some(idx) = mid_matched_evict {
            self.adjust_after_mid_visible_evict(idx);
        }
        if let Some(evicted) = evicted_row {
            if active && self.is_bookmark_row(evicted.row_id) && !self.row_passes_filters(&evicted)
            {
                self.remove_matched_row_id(evicted.row_id);
            }
        }

        let stream = self.stream_mut();
        if matches {
            stream.rows.push_back(row.clone());
            stream.matched.push_back(row);
            let len = stream.matched.len();
            self.index.visible = Visible::All { len };
        } else {
            stream.rows.push_back(row);
            if !active {
                let len = stream.rows.len();
                self.index.visible = Visible::All { len };
            }
        }
        self.follow_tick();
        self.match_stats_stale = true;
    }

    /// O(1) cursor/viewport adjustment after the front row of the identity
    /// `view_source` was evicted and a replacement is about to be (or was)
    /// appended — `Visible::All.len` is unchanged across the pop+push pair.
    fn adjust_after_identity_front_evict(&mut self) {
        // No visible slots ⇒ nothing was on-screen at source index 0.
        if self.index.visible.is_empty() {
            return;
        }
        // Identity: source index 0 was always visible slot 0 when non-empty.
        if self.list_offset > 0 {
            self.list_offset -= 1;
        }
        if self.cursor > 0 {
            self.cursor -= 1;
        }
        // `visual_anchor` shares `cursor`'s coordinate space (index into
        // visible slots). Evicting the oldest visible row shifts that space.
        match self.visual_anchor {
            Some(0) => self.visual_anchor = None,
            Some(a) => self.visual_anchor = Some(a - 1),
            None => {}
        }
    }

    pub(super) fn adjust_after_mid_visible_evict(&mut self, idx: usize) {
        if self.cursor > idx {
            self.cursor -= 1;
        }
        if self.list_offset > idx {
            self.list_offset -= 1;
        }
        match self.visual_anchor {
            Some(a) if a == idx => self.visual_anchor = None,
            Some(a) if a > idx => self.visual_anchor = Some(a - 1),
            _ => {}
        }
        if self.cursor >= self.index.visible.len() {
            self.cursor = self.index.visible.len().saturating_sub(1);
        }
        self.match_stats_stale = true;
    }

    fn merge_file_filter_hits(&mut self, hits: &[usize]) {
        let pins = self.alive_bookmark_file_line_indices();
        match &mut self.index.visible {
            Visible::Subset(v) => {
                merge_sorted_unique(v, hits);
                merge_sorted_unique(v, &pins);
            }
            _ => {
                let mut v = hits.to_vec();
                merge_sorted_unique(&mut v, &pins);
                self.index.visible = Visible::Subset(v);
            }
        }
    }

    /// Clear buffered live log rows for Ctrl-L: drops `rows` / `matched` /
    /// `visible` and bookmarks, keeps filter/highlight/exclude/lock, resumes
    /// following, and flashes `CLEARED`.
    pub fn clear_buffered_logs(&mut self) {
        if let Some(stream) = self.index.store.as_stream_mut() {
            stream.clear();
        }
        self.index.visible.clear();
        self.clear_bookmarks();
        self.clear_visual();
        self.pending = Pending::None;
        self.cursor = 0;
        self.list_offset = 0;
        self.match_stats_stale = true;
        self.resume_following();
        self.set_flash("CLEARED");
    }

    /// Clear live disconnect state after a successful respawn; keep buffers.
    pub fn mark_live_reconnected(&mut self) {
        self.ingest_done = false;
        self.set_flash("RECONNECTED");
    }
}
