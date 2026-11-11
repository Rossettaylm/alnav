use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self};
use std::sync::{Arc, RwLock};
use std::thread::{self, JoinHandle};

use memchr::memchr_iter;
use memmap2::Mmap;

use crate::model::{is_severe_row, EntryRow};

use super::{FileEvent, FilterPred, LineSpan};

/// Indexer reports progress every this many newlines.
const INDEX_PROGRESS_EVERY: usize = 4096;
/// Filter scanner first batch size (quick first paint on large files).
const FILTER_BATCH_FIRST_HITS: usize = 64;
/// Filter scanner subsequent batch size after the first flush.
const FILTER_BATCH_HITS: usize = 2048;
/// Filter scanner processes this many lines per inner loop before checking cancel.
const FILTER_CHUNK_LINES: usize = 8192;

/// Whether the filter worker should send the current hit batch to the UI.
///
/// - Before any batch has been sent: flush at [`FILTER_BATCH_FIRST_HITS`].
/// - After: flush at [`FILTER_BATCH_HITS`].
/// - At chunk end: flush any non-empty remainder (sparse filters).
pub(super) fn should_flush_filter_batch(batch_len: usize, sent_any: bool, chunk_end: bool) -> bool {
    if batch_len == 0 {
        return false;
    }
    if chunk_end {
        return true;
    }
    let threshold = if sent_any {
        FILTER_BATCH_HITS
    } else {
        FILTER_BATCH_FIRST_HITS
    };
    batch_len >= threshold
}

pub(super) fn scan_line_spans(mmap: &[u8]) -> Vec<LineSpan> {
    let mut spans = Vec::new();
    let mut start = 0usize;
    for nl in memchr_iter(b'\n', mmap) {
        let mut end = nl;
        if end > start && mmap[end - 1] == b'\r' {
            end -= 1;
        }
        spans.push(LineSpan {
            start: start as u64,
            len: (end - start) as u32,
        });
        start = nl + 1;
    }
    if start < mmap.len() {
        let mut end = mmap.len();
        if end > start && mmap[end - 1] == b'\r' {
            end -= 1;
        }
        spans.push(LineSpan {
            start: start as u64,
            len: (end - start) as u32,
        });
    }
    spans
}

pub(super) fn index_file_background(
    mmap: Arc<Mmap>,
    lines: Arc<RwLock<Vec<LineSpan>>>,
    indexed_bytes: Arc<AtomicUsize>,
    index_done: Arc<AtomicBool>,
    tx: mpsc::Sender<FileEvent>,
) {
    let data: &[u8] = &mmap;
    let mut start = 0usize;
    let mut since_progress = 0usize;
    for nl in memchr_iter(b'\n', data) {
        let mut end = nl;
        if end > start && data[end - 1] == b'\r' {
            end -= 1;
        }
        let span = LineSpan {
            start: start as u64,
            len: (end - start) as u32,
        };
        {
            let mut g = lines.write().expect("lines lock");
            g.push(span);
        }
        start = nl + 1;
        indexed_bytes.store(start, Ordering::Relaxed);
        since_progress += 1;
        if since_progress >= INDEX_PROGRESS_EVERY {
            since_progress = 0;
            let count = lines.read().expect("lines lock").len();
            if tx
                .send(FileEvent::IndexProgress {
                    line_count: count,
                    bytes_done: start,
                })
                .is_err()
            {
                return;
            }
        }
    }
    if start < data.len() {
        let mut end = data.len();
        if end > start && data[end - 1] == b'\r' {
            end -= 1;
        }
        let span = LineSpan {
            start: start as u64,
            len: (end - start) as u32,
        };
        lines.write().expect("lines lock").push(span);
    }
    indexed_bytes.store(data.len(), Ordering::Relaxed);
    let count = lines.read().expect("lines lock").len();
    index_done.store(true, Ordering::Release);
    let _ = tx.send(FileEvent::IndexDone { line_count: count });
}

/// Run filter scan on a worker; send batches through `tx`.
/// Continues until `index_done` and all currently indexed lines are scanned.
pub(super) fn spawn_filter_scan(
    mmap: Arc<Mmap>,
    lines: Arc<RwLock<Vec<LineSpan>>>,
    index_done: Arc<AtomicBool>,
    cancel: Arc<AtomicBool>,
    filter_scanned: Arc<AtomicUsize>,
    filter_done_flag: Arc<AtomicBool>,
    gen: u64,
    pred: FilterPred,
    tx: mpsc::Sender<FileEvent>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        let mut i = 0usize;
        let mut batch = Vec::with_capacity(FILTER_BATCH_FIRST_HITS);
        let mut sent_any = false;
        loop {
            if cancel.load(Ordering::Acquire) {
                return;
            }
            let n = lines.read().expect("lines lock").len();
            if i >= n {
                if index_done.load(Ordering::Acquire) {
                    break;
                }
                thread::sleep(std::time::Duration::from_millis(2));
                continue;
            }
            let end = (i + FILTER_CHUNK_LINES).min(n);
            while i < end {
                if cancel.load(Ordering::Acquire) {
                    return;
                }
                let span = lines.read().expect("lines lock")[i];
                let start = span.start as usize;
                let end_b = start.saturating_add(span.len as usize).min(mmap.len());
                let cow = String::from_utf8_lossy(&mmap[start..end_b]);
                let mut row = EntryRow::from_line_or_raw(cow.as_ref());
                row.row_id = (i as u64).saturating_add(1);
                // Match `parse_span`: from_line*_ leaves severe=false; ViewFocus.severe
                // (`fe`) and any future pred that reads `row.severe` need it set here.
                row.severe = is_severe_row(&row);
                if pred(&row) {
                    batch.push(i);
                    if should_flush_filter_batch(batch.len(), sent_any, false) {
                        let hits = std::mem::take(&mut batch);
                        if tx
                            .send(FileEvent::FilterBatch {
                                gen,
                                hits,
                                scanned: i + 1,
                            })
                            .is_err()
                        {
                            return;
                        }
                        sent_any = true;
                    }
                }
                i += 1;
                filter_scanned.store(i, Ordering::Relaxed);
            }
            // Chunk boundary: flush sparse remainder so UI paints before Done.
            if should_flush_filter_batch(batch.len(), sent_any, true) {
                let hits = std::mem::take(&mut batch);
                if tx
                    .send(FileEvent::FilterBatch {
                        gen,
                        hits,
                        scanned: i,
                    })
                    .is_err()
                {
                    return;
                }
                sent_any = true;
            }
        }
        if !batch.is_empty() {
            let hits = std::mem::take(&mut batch);
            if tx
                .send(FileEvent::FilterBatch {
                    gen,
                    hits,
                    scanned: i,
                })
                .is_err()
            {
                return;
            }
        }
        filter_done_flag.store(true, Ordering::Release);
        let _ = tx.send(FileEvent::FilterDone { gen, scanned: i });
    })
}
