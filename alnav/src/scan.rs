//! Background Vis-domain scans for File mode (highlight hit index + severe prefetch).
//!
//! Filter scanning stays in [`crate::store::FileStore`]; this module owns the
//! shared domain snapshot and worker loops that must not run O(visible) `row_at`
//! on the UI thread.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, RwLock};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use memmap2::Mmap;

use crate::fuzzy;
use crate::model::{is_severe_row, EntryRow};
use crate::store::{FileEvent, LineSpan};

/// How many vis slots a highlight worker processes before checking cancel / growth.
const HIGHLIGHT_CHUNK: usize = 4096;
/// Yield a highlight batch after this many hits.
const HIGHLIGHT_BATCH_HITS: usize = 2048;
/// Severe prefetch lines per cancel check.
const SEVERE_CHUNK: usize = 8192;

/// Growing visible-domain snapshot shared between UI and highlight worker.
///
/// - Identity (`All`): `identity_len` is the visible length; vis slot `i` → source `i`.
/// - Subset: `subset[vis_i]` is the file line index.
#[derive(Debug)]
pub struct HighlightDomain {
    pub identity: AtomicBool,
    pub identity_len: AtomicUsize,
    pub subset: RwLock<Vec<usize>>,
    /// When true, domain will not grow; worker may finish once caught up.
    pub sealed: AtomicBool,
}

impl HighlightDomain {
    pub fn identity(len: usize) -> Arc<Self> {
        Arc::new(Self {
            identity: AtomicBool::new(true),
            identity_len: AtomicUsize::new(len),
            subset: RwLock::new(Vec::new()),
            sealed: AtomicBool::new(false),
        })
    }

    pub fn subset(hits: Vec<usize>) -> Arc<Self> {
        Arc::new(Self {
            identity: AtomicBool::new(false),
            identity_len: AtomicUsize::new(0),
            subset: RwLock::new(hits),
            sealed: AtomicBool::new(false),
        })
    }

    pub fn len(&self) -> usize {
        if self.identity.load(Ordering::Acquire) {
            self.identity_len.load(Ordering::Acquire)
        } else {
            self.subset.read().expect("subset").len()
        }
    }

    pub fn source_at(&self, vis_i: usize) -> Option<usize> {
        if self.identity.load(Ordering::Acquire) {
            let n = self.identity_len.load(Ordering::Acquire);
            if vis_i < n {
                Some(vis_i)
            } else {
                None
            }
        } else {
            self.subset.read().expect("subset").get(vis_i).copied()
        }
    }

    pub fn set_identity_len(&self, len: usize) {
        self.identity.store(true, Ordering::Release);
        self.identity_len.store(len, Ordering::Release);
    }

    pub fn extend_subset(&self, hits: &[usize]) {
        if hits.is_empty() {
            return;
        }
        self.identity.store(false, Ordering::Release);
        self.subset.write().expect("subset").extend_from_slice(hits);
    }

    pub fn replace_subset(&self, hits: Vec<usize>) {
        self.identity.store(false, Ordering::Release);
        *self.subset.write().expect("subset") = hits;
    }

    pub fn seal(&self) {
        self.sealed.store(true, Ordering::Release);
    }

    pub fn is_sealed(&self) -> bool {
        self.sealed.load(Ordering::Acquire)
    }
}

/// UI-side highlight hit index (Vis indices, sorted ascending).
#[derive(Debug, Default, Clone)]
pub struct HighlightScanState {
    pub gen: u64,
    pub hits: Vec<usize>,
    pub scanned_vis: usize,
    pub done: bool,
}

impl HighlightScanState {
    pub fn clear(&mut self) {
        self.hits.clear();
        self.scanned_vis = 0;
        self.done = false;
    }

    /// Match stats from the hit index — O(log n), no row parse.
    pub fn match_stats(&self, cursor: usize) -> (Option<usize>, usize) {
        let total = self.hits.len();
        match self.hits.binary_search(&cursor) {
            Ok(i) => (Some(i + 1), total),
            Err(_) => (None, total),
        }
    }

    /// Next/prev hit vis index **without** wrapscan (skip current).
    ///
    /// At either end with no further hit in `dir`, returns `None` (caller may
    /// flash `NO MORE`). While Inc is still running (`!done`), the same bounded
    /// rule applies — do not jump past the last/first *known* hit.
    pub fn find_next(&self, cursor: usize, dir: i8) -> Option<usize> {
        let hits = &self.hits;
        if hits.is_empty() {
            return None;
        }
        if dir >= 0 {
            let i = hits.partition_point(|&h| h <= cursor);
            if i < hits.len() {
                Some(hits[i])
            } else {
                None
            }
        } else {
            let i = hits.partition_point(|&h| h < cursor);
            if i > 0 {
                Some(hits[i - 1])
            } else {
                None
            }
        }
    }

    pub fn first_hit(&self) -> Option<usize> {
        self.hits.first().copied()
    }
}

fn parse_line_at(mmap: &Mmap, lines: &[LineSpan], i: usize) -> Option<EntryRow> {
    let span = *lines.get(i)?;
    let start = span.start as usize;
    let end = start.saturating_add(span.len as usize).min(mmap.len());
    let cow = String::from_utf8_lossy(&mmap[start..end]);
    let mut row = EntryRow::from_line_or_raw(cow.as_ref());
    row.row_id = (i as u64).saturating_add(1);
    Some(row)
}

/// Spawn Vis-domain highlight scan; sends [`FileEvent::HighlightBatch`]/ [`FileEvent::HighlightDone`].
pub fn spawn_highlight_scan(
    mmap: Arc<Mmap>,
    lines: Arc<RwLock<Vec<LineSpan>>>,
    domain: Arc<HighlightDomain>,
    cancel: Arc<AtomicBool>,
    scanned_out: Arc<AtomicUsize>,
    done_flag: Arc<AtomicBool>,
    gen: u64,
    pattern: String,
    tx: Sender<FileEvent>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        let mut i = 0usize;
        let mut batch = Vec::with_capacity(HIGHLIGHT_BATCH_HITS);
        loop {
            if cancel.load(Ordering::Acquire) {
                return;
            }
            let n = domain.len();
            if i >= n {
                if domain.is_sealed() {
                    break;
                }
                thread::sleep(Duration::from_millis(2));
                continue;
            }
            let end = (i + HIGHLIGHT_CHUNK).min(n);
            while i < end {
                if cancel.load(Ordering::Acquire) {
                    return;
                }
                let Some(src) = domain.source_at(i) else {
                    i += 1;
                    scanned_out.store(i, Ordering::Relaxed);
                    continue;
                };
                let hit = {
                    let guard = lines.read().expect("lines");
                    if let Some(row) = parse_line_at(&mmap, &guard, src) {
                        fuzzy::matches_search_row(&row, &pattern)
                    } else {
                        false
                    }
                };
                if hit {
                    batch.push(i);
                    if batch.len() >= HIGHLIGHT_BATCH_HITS {
                        let hits = std::mem::take(&mut batch);
                        if tx
                            .send(FileEvent::HighlightBatch {
                                gen,
                                hits,
                                scanned: i + 1,
                            })
                            .is_err()
                        {
                            return;
                        }
                    }
                }
                i += 1;
                scanned_out.store(i, Ordering::Relaxed);
            }
        }
        if !batch.is_empty() {
            let hits = std::mem::take(&mut batch);
            if tx
                .send(FileEvent::HighlightBatch {
                    gen,
                    hits,
                    scanned: i,
                })
                .is_err()
            {
                return;
            }
        }
        done_flag.store(true, Ordering::Release);
        let _ = tx.send(FileEvent::HighlightDone { gen, scanned: i });
    })
}

/// Fill `severe_cache` in the background (indexed lines only; waits for growth).
pub fn spawn_severe_prefetch(
    mmap: Arc<Mmap>,
    lines: Arc<RwLock<Vec<LineSpan>>>,
    index_done: Arc<AtomicBool>,
    cancel: Arc<AtomicBool>,
    severe_cache: Arc<RwLock<Vec<Option<bool>>>>,
    scanned_out: Arc<AtomicUsize>,
    done_flag: Arc<AtomicBool>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        let mut i = 0usize;
        loop {
            if cancel.load(Ordering::Acquire) {
                return;
            }
            let n = lines.read().expect("lines").len();
            {
                let mut cache = severe_cache.write().expect("severe");
                if cache.len() < n {
                    cache.resize(n, None);
                }
            }
            if i >= n {
                if index_done.load(Ordering::Acquire) {
                    break;
                }
                thread::sleep(Duration::from_millis(2));
                continue;
            }
            let end = (i + SEVERE_CHUNK).min(n);
            while i < end {
                if cancel.load(Ordering::Acquire) {
                    return;
                }
                let already = {
                    let cache = severe_cache.read().expect("severe");
                    cache.get(i).copied().flatten()
                };
                if already.is_none() {
                    let row = {
                        let guard = lines.read().expect("lines");
                        parse_line_at(&mmap, &guard, i)
                    };
                    if let Some(row) = row {
                        let v = is_severe_row(&row);
                        let mut cache = severe_cache.write().expect("severe");
                        if cache.len() <= i {
                            cache.resize(i + 1, None);
                        }
                        cache[i] = Some(v);
                    }
                }
                i += 1;
                scanned_out.store(i, Ordering::Relaxed);
            }
        }
        done_flag.store(true, Ordering::Release);
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highlight_stats_and_find_next() {
        let mut st = HighlightScanState {
            gen: 1,
            hits: vec![1, 3, 7],
            scanned_vis: 10,
            done: true,
        };
        assert_eq!(st.match_stats(3), (Some(2), 3));
        assert_eq!(st.match_stats(0), (None, 3));
        assert_eq!(st.find_next(0, 1), Some(1));
        assert_eq!(st.find_next(1, 1), Some(3));
        assert_eq!(st.find_next(7, 1), None); // no wrap when done
        assert_eq!(st.find_next(3, -1), Some(1));
        assert_eq!(st.find_next(1, -1), None); // no wrap back when done
        assert_eq!(st.first_hit(), Some(1));
        st.done = false;
        assert_eq!(st.find_next(7, 1), None); // no wrap while Inc
        assert_eq!(st.find_next(1, -1), None);
        st.clear();
        assert!(st.hits.is_empty());
        assert!(!st.done);
    }

    #[test]
    fn domain_identity_and_subset_extend() {
        let d = HighlightDomain::identity(3);
        assert_eq!(d.len(), 3);
        assert_eq!(d.source_at(2), Some(2));
        d.set_identity_len(5);
        assert_eq!(d.len(), 5);
        d.replace_subset(vec![0, 2]);
        assert_eq!(d.len(), 2);
        assert_eq!(d.source_at(1), Some(2));
        d.extend_subset(&[4]);
        assert_eq!(d.len(), 3);
        assert_eq!(d.source_at(2), Some(4));
        d.seal();
        assert!(d.is_sealed());
    }
}
