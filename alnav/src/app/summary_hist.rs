//! Summary and histogram panels with their background report jobs.

use super::{App, SummaryJobMsg, SummaryView, Visible};
use crate::hist_panel;
use crate::hist_panel::{HistJobMsg, HistView};
use crate::model::EntryRow;
use crate::store::RowStore;
use std::sync::Arc;

/// File-mode summary job target: the physical line indices to scan, captured
/// once at open time (identity range or a filtered subset).
enum SummaryTarget {
    All(usize),
    Subset(Vec<usize>),
}

fn spawn_file_summary_job(
    mmap: Arc<memmap2::Mmap>,
    lines: Arc<std::sync::RwLock<Vec<crate::store::LineSpan>>>,
    target: SummaryTarget,
    gen: u64,
    tx: std::sync::mpsc::Sender<SummaryJobMsg>,
) {
    std::thread::spawn(move || {
        let mut summary = alnav::summary::Summary::new();
        let mut count = 0usize;
        let indices: Box<dyn Iterator<Item = usize>> = match target {
            SummaryTarget::All(len) => Box::new(0..len),
            SummaryTarget::Subset(hits) => Box::new(hits.into_iter()),
        };
        for i in indices {
            let row = {
                let guard = lines.read().expect("lines");
                crate::scan::parse_line_at(&mmap, &guard, i)
            };
            if let Some(row) = row {
                summary.record(&row.as_log_entry());
                count += 1;
            }
        }
        let report = summary.into_report(count);
        let _ = tx.send(SummaryJobMsg { gen, report });
    });
}

fn spawn_stream_summary_job(
    rows: Vec<EntryRow>,
    gen: u64,
    tx: std::sync::mpsc::Sender<SummaryJobMsg>,
) {
    std::thread::spawn(move || {
        let mut summary = alnav::summary::Summary::new();
        for row in &rows {
            summary.record(&row.as_log_entry());
        }
        let report = summary.into_report(rows.len());
        let _ = tx.send(SummaryJobMsg { gen, report });
    });
}

fn spawn_file_hist_job(
    mmap: Arc<memmap2::Mmap>,
    lines: Arc<std::sync::RwLock<Vec<crate::store::LineSpan>>>,
    target: SummaryTarget,
    interval_secs: u64,
    gen: u64,
    tx: std::sync::mpsc::Sender<HistJobMsg>,
) {
    std::thread::spawn(move || {
        let mut pairs = Vec::new();
        match target {
            SummaryTarget::All(len) => {
                for i in 0..len {
                    let row = {
                        let guard = lines.read().expect("lines");
                        crate::scan::parse_line_at(&mmap, &guard, i)
                    };
                    if let Some(row) = row {
                        pairs.push((i, row));
                    }
                }
            }
            SummaryTarget::Subset(hits) => {
                for (vis_i, src) in hits.into_iter().enumerate() {
                    let row = {
                        let guard = lines.read().expect("lines");
                        crate::scan::parse_line_at(&mmap, &guard, src)
                    };
                    if let Some(row) = row {
                        pairs.push((vis_i, row));
                    }
                }
            }
        }
        let report = hist_panel::build_report(pairs, interval_secs);
        let _ = tx.send(HistJobMsg { gen, report });
    });
}

fn spawn_stream_hist_job(
    pairs: Vec<(usize, EntryRow)>,
    interval_secs: u64,
    gen: u64,
    tx: std::sync::mpsc::Sender<HistJobMsg>,
) {
    std::thread::spawn(move || {
        let report = hist_panel::build_report(pairs, interval_secs);
        let _ = tx.send(HistJobMsg { gen, report });
    });
}

impl App {
    /// Whether the summary panel is open (Loading or Ready).
    pub fn summary_open(&self) -> bool {
        !matches!(self.summary_view, SummaryView::Closed)
    }

    /// Leader `i`: snapshot the current `visible` into a background summary
    /// job. Bumps `summary_gen` so a stale in-flight result (from a prior
    /// open) is dropped when it arrives. Does not change `following`.
    pub fn open_summary_panel(&mut self) {
        self.summary_gen = self.summary_gen.wrapping_add(1);
        let gen = self.summary_gen;
        self.summary_view = SummaryView::Loading;
        self.summary_scroll = 0;
        match &self.store {
            RowStore::File(f) => {
                let (mmap, lines) = f.scan_snapshot();
                let target = match &self.visible {
                    Visible::All { len } => SummaryTarget::All(*len),
                    Visible::Subset(v) => SummaryTarget::Subset(v.clone()),
                };
                spawn_file_summary_job(mmap, lines, target, gen, self.summary_tx.clone());
            }
            RowStore::Stream(s) => {
                let rows: Vec<EntryRow> = s
                    .view_source(self.filter_active())
                    .iter()
                    .cloned()
                    .collect();
                spawn_stream_summary_job(rows, gen, self.summary_tx.clone());
            }
        }
    }

    /// Close the summary panel (Esc / toggle re-press). Bumps `summary_gen`
    /// so any in-flight background result is discarded on arrival. Does not
    /// resume following (same convention as Detail/Help).
    pub fn close_summary_panel(&mut self) {
        self.summary_gen = self.summary_gen.wrapping_add(1);
        self.summary_view = SummaryView::Closed;
        self.summary_scroll = 0;
    }

    /// Drain finished summary jobs (call each frame). Results whose `gen`
    /// no longer matches the current request are silently dropped.
    pub fn poll_summary_job(&mut self) {
        while let Ok(msg) = self.summary_rx.try_recv() {
            if msg.gen != self.summary_gen {
                continue;
            }
            self.summary_view = SummaryView::Ready(msg.report);
        }
    }

    /// Block until the in-flight summary job completes (tests).
    pub fn flush_summary_job(&mut self, timeout: std::time::Duration) {
        let deadline = std::time::Instant::now() + timeout;
        while matches!(self.summary_view, SummaryView::Loading) {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                break;
            }
            match self.summary_rx.recv_timeout(remaining) {
                Ok(msg) => {
                    if msg.gen == self.summary_gen {
                        self.summary_view = SummaryView::Ready(msg.report);
                    }
                }
                Err(_) => break,
            }
        }
    }

    /// Scroll the summary panel body; only active while `Ready`. Upper bound
    /// is clamped at render time against the built content length.
    pub fn hist_open(&self) -> bool {
        !matches!(self.hist_view, HistView::Closed)
    }

    fn hist_interval_default(&self) -> u64 {
        let catalog =
            crate::time_panel::DateCatalog::from_rows(self.sample_rows_for_dates().iter());
        if catalog.dates.len() != 1 {
            return hist_panel::pick_interval_from_span(u64::MAX / 2);
        }
        let d = &catalog.dates[0];
        let span = match (
            alnav::histogram::hms_to_secs(&d.max_hms),
            alnav::histogram::hms_to_secs(&d.min_hms),
        ) {
            (Some(max), Some(min)) => max.saturating_sub(min),
            _ => 60,
        };
        hist_panel::pick_interval_from_span(span)
    }

    fn spawn_hist_job(&mut self, interval_secs: u64) {
        self.hist_gen = self.hist_gen.wrapping_add(1);
        let gen = self.hist_gen;
        self.hist_view = HistView::Loading { interval_secs };
        match &self.store {
            RowStore::File(f) => {
                let (mmap, lines) = f.scan_snapshot();
                let target = match &self.visible {
                    Visible::All { len } => SummaryTarget::All(*len),
                    Visible::Subset(v) => SummaryTarget::Subset(v.clone()),
                };
                spawn_file_hist_job(
                    mmap,
                    lines,
                    target,
                    interval_secs,
                    gen,
                    self.hist_tx.clone(),
                );
            }
            RowStore::Stream(_) => {
                let n = self.visible.len();
                let mut pairs = Vec::with_capacity(n);
                for i in 0..n {
                    if let Some(row) = self.row_at(i) {
                        pairs.push((i, row.into_owned()));
                    }
                }
                spawn_stream_hist_job(pairs, interval_secs, gen, self.hist_tx.clone());
            }
        }
    }

    /// `th`: open the time histogram over current `visible`. File export only.
    pub fn open_hist_panel(&mut self) -> bool {
        self.pending_time = false;
        if !self.is_file_mode() {
            return false;
        }
        if !self.has_time_date_candidates() {
            self.set_flash("NO DATES");
            return false;
        }
        self.following = false;
        self.time_panel = None;
        self.close_summary_panel();
        self.hist_cursor = 0;
        self.spawn_hist_job(self.hist_interval_default());
        true
    }

    pub fn close_hist_panel(&mut self) {
        self.hist_gen = self.hist_gen.wrapping_add(1);
        self.hist_view = HistView::Closed;
        self.hist_cursor = 0;
    }

    pub fn poll_hist_job(&mut self) {
        while let Ok(msg) = self.hist_rx.try_recv() {
            if msg.gen != self.hist_gen {
                continue;
            }
            if msg.report.buckets.is_empty() {
                self.hist_view = HistView::Closed;
                self.hist_restore_key = None;
                self.set_flash("NO DATES");
                continue;
            }
            self.hist_cursor = if let Some(key) = self.hist_restore_key.take() {
                msg.report.index_for_key(&key)
            } else {
                self.hist_cursor.min(msg.report.buckets.len() - 1)
            };
            self.hist_view = HistView::Ready(msg.report);
        }
    }

    pub fn flush_hist_job(&mut self, timeout: std::time::Duration) {
        let deadline = std::time::Instant::now() + timeout;
        while matches!(self.hist_view, HistView::Loading { .. }) {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                break;
            }
            match self.hist_rx.recv_timeout(remaining) {
                Ok(msg) => {
                    if msg.gen == self.hist_gen {
                        if msg.report.buckets.is_empty() {
                            self.hist_view = HistView::Closed;
                            self.hist_restore_key = None;
                            self.set_flash("NO DATES");
                        } else {
                            self.hist_cursor = if let Some(key) = self.hist_restore_key.take() {
                                msg.report.index_for_key(&key)
                            } else {
                                self.hist_cursor.min(msg.report.buckets.len() - 1)
                            };
                            self.hist_view = HistView::Ready(msg.report);
                        }
                    }
                }
                Err(_) => break,
            }
        }
    }

    pub fn move_hist_cursor(&mut self, delta: isize) {
        let HistView::Ready(report) = &self.hist_view else {
            return;
        };
        let n = report.buckets.len() as isize;
        if n == 0 {
            return;
        }
        let next = (self.hist_cursor as isize + delta).clamp(0, n - 1);
        self.hist_cursor = next as usize;
    }

    pub fn jump_hist_cursor(&mut self, to_last: bool) {
        let HistView::Ready(report) = &self.hist_view else {
            return;
        };
        if report.buckets.is_empty() {
            return;
        }
        self.hist_cursor = if to_last { report.buckets.len() - 1 } else { 0 };
    }

    pub fn zoom_hist(&mut self, finer: bool) {
        let current = match &self.hist_view {
            HistView::Ready(r) => r.interval_secs,
            HistView::Loading { interval_secs } => *interval_secs,
            HistView::Closed => return,
        };
        let keep_key = match &self.hist_view {
            HistView::Ready(r) => r.buckets.get(self.hist_cursor).map(|b| b.key.clone()),
            _ => None,
        };
        let next = alnav::histogram::cycle_interval_secs(current, finer);
        if next == current {
            return;
        }
        self.hist_restore_key = keep_key;
        self.spawn_hist_job(next);
    }

    pub fn submit_hist_jump(&mut self) {
        let HistView::Ready(report) = &self.hist_view else {
            return;
        };
        let Some(bucket) = report.buckets.get(self.hist_cursor) else {
            return;
        };
        let idx = bucket.jump_visible();
        self.close_hist_panel();
        if self.visible.is_empty() {
            return;
        }
        self.following = false;
        self.cursor = idx.min(self.visible.len() - 1);
        self.match_stats_stale = true;
    }

    pub fn apply_hist_window(&mut self) {
        let HistView::Ready(report) = &self.hist_view else {
            return;
        };
        let Some(bucket) = report.buckets.get(self.hist_cursor) else {
            return;
        };
        let Some(bound) = bucket.time_bound(report.interval_secs) else {
            return;
        };
        self.close_hist_panel();
        self.apply_time_bound(bound);
    }

    /// Scroll the summary panel body; only active while `Ready`. Upper bound
    /// is clamped at render time against the built content length.
    pub fn scroll_summary(&mut self, delta: isize) {
        if !matches!(self.summary_view, SummaryView::Ready(_)) {
            return;
        }
        if delta < 0 {
            self.summary_scroll = self.summary_scroll.saturating_sub((-delta) as usize);
        } else {
            self.summary_scroll = self.summary_scroll.saturating_add(delta as usize);
        }
    }
}
