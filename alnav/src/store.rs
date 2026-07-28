//! Row storage backends: mmap file (`FileStore`) and live stream (`StreamStore`).
//!
//! Readers go through [`RowStore::row_at_source`] / [`App::row_at`](crate::app::App::row_at)
//! returning [`RowRef`] (borrowed for stream, owned lazy-parse for file).

use std::collections::VecDeque;
use std::fs::File;
use std::io;
use std::num::NonZeroUsize;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::{self, JoinHandle};

use lru::LruCache;
use memchr::memchr_iter;
use memmap2::Mmap;

use crate::model::{is_severe_row, EntryRow};
use crate::scan::{self, HighlightDomain};

/// Parse-cache capacity for file lazy rows (viewport + minimap reuse).
const FILE_PARSE_LRU: usize = 256;
/// Indexer reports progress every this many newlines.
const INDEX_PROGRESS_EVERY: usize = 4096;
/// Filter scanner yields a batch every this many hits.
const FILTER_BATCH_HITS: usize = 2048;
/// Filter scanner processes this many lines per inner loop before checking cancel.
const FILTER_CHUNK_LINES: usize = 8192;
/// Vocab sampler: hard cap on lines parsed (keeps IndexDone off the UI
/// critical path for multi-million-line files).
const VOCAB_MAX_SAMPLES: usize = 4096;

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

/// Progress snapshot for status bar / tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FileProgress {
    pub indexed_lines: usize,
    pub indexed_bytes: usize,
    pub file_bytes: usize,
    pub index_done: bool,
    pub filter_scanned: usize,
    pub filter_done: bool,
    /// Active filter generation (0 = none / inactive filter).
    pub filter_gen: u64,
    pub highlight_scanned: usize,
    pub highlight_done: bool,
    pub highlight_gen: u64,
    pub severe_scanned: usize,
    pub severe_done: bool,
}

/// Messages from background file workers to the UI thread.
#[derive(Debug)]
pub enum FileEvent {
    /// Indexer appended lines; `line_count` is the new total.
    IndexProgress {
        line_count: usize,
        bytes_done: usize,
    },
    IndexDone {
        line_count: usize,
    },
    /// Incremental filter hits for generation `gen`.
    FilterBatch {
        gen: u64,
        hits: Vec<usize>,
        scanned: usize,
    },
    FilterDone {
        gen: u64,
        scanned: usize,
    },
    /// Incremental highlight hits (Vis indices) for generation `gen`.
    HighlightBatch {
        gen: u64,
        hits: Vec<usize>,
        scanned: usize,
    },
    HighlightDone {
        gen: u64,
        scanned: usize,
    },
}

/// Predicate used by the background filter scanner (cloned into the worker).
pub type FilterPred = Arc<dyn Fn(&EntryRow) -> bool + Send + Sync>;

/// mmap + line index + lazy parse file backend.
pub struct FileStore {
    path: PathBuf,
    mmap: Arc<Mmap>,
    lines: Arc<RwLock<Vec<LineSpan>>>,
    file_bytes: usize,
    indexed_bytes: Arc<AtomicUsize>,
    index_done: Arc<AtomicBool>,
    event_tx: Sender<FileEvent>,
    events: Receiver<FileEvent>,
    _index_handle: Option<JoinHandle<()>>,
    filter_handle: Option<JoinHandle<()>>,
    filter_cancel: Arc<AtomicBool>,
    filter_gen: Arc<AtomicU64>,
    filter_scanned: Arc<AtomicUsize>,
    filter_done: Arc<AtomicBool>,
    highlight_handle: Option<JoinHandle<()>>,
    highlight_cancel: Arc<AtomicBool>,
    highlight_gen: Arc<AtomicU64>,
    highlight_scanned: Arc<AtomicUsize>,
    highlight_done: Arc<AtomicBool>,
    highlight_domain: Option<Arc<HighlightDomain>>,
    _severe_handle: Option<JoinHandle<()>>,
    _severe_cancel: Arc<AtomicBool>,
    severe_scanned: Arc<AtomicUsize>,
    severe_done: Arc<AtomicBool>,
    parse_lru: Mutex<LruCache<usize, EntryRow>>,
    /// Lazy / prefetched severe flags keyed by line index; `None` = unknown.
    severe_cache: Arc<RwLock<Vec<Option<bool>>>>,
    vocab_started: AtomicBool,
}

impl FileStore {
    /// Open path, mmap, and start background indexing (Phase C).
    pub fn open(path: impl Into<PathBuf>) -> io::Result<Self> {
        let path = path.into();
        let file = File::open(&path)?;
        let mmap = unsafe { Mmap::map(&file)? };
        let file_bytes = mmap.len();
        let mmap = Arc::new(mmap);
        let lines = Arc::new(RwLock::new(Vec::new()));
        let indexed_bytes = Arc::new(AtomicUsize::new(0));
        let index_done = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();

        let mmap_bg = Arc::clone(&mmap);
        let lines_bg = Arc::clone(&lines);
        let indexed_bytes_bg = Arc::clone(&indexed_bytes);
        let index_done_bg = Arc::clone(&index_done);
        let tx_bg = tx.clone();
        let index_handle = thread::spawn(move || {
            index_file_background(mmap_bg, lines_bg, indexed_bytes_bg, index_done_bg, tx_bg);
        });

        let severe_cache = Arc::new(RwLock::new(Vec::new()));
        let severe_cancel = Arc::new(AtomicBool::new(false));
        let severe_scanned = Arc::new(AtomicUsize::new(0));
        let severe_done = Arc::new(AtomicBool::new(false));
        let _severe_handle = Some(scan::spawn_severe_prefetch(
            Arc::clone(&mmap),
            Arc::clone(&lines),
            Arc::clone(&index_done),
            Arc::clone(&severe_cancel),
            Arc::clone(&severe_cache),
            Arc::clone(&severe_scanned),
            Arc::clone(&severe_done),
        ));

        Ok(Self {
            path,
            mmap,
            lines,
            file_bytes,
            indexed_bytes,
            index_done,
            event_tx: tx,
            events: rx,
            _index_handle: Some(index_handle),
            filter_handle: None,
            filter_cancel: Arc::new(AtomicBool::new(false)),
            filter_gen: Arc::new(AtomicU64::new(0)),
            filter_scanned: Arc::new(AtomicUsize::new(0)),
            filter_done: Arc::new(AtomicBool::new(true)),
            highlight_handle: None,
            highlight_cancel: Arc::new(AtomicBool::new(false)),
            highlight_gen: Arc::new(AtomicU64::new(0)),
            highlight_scanned: Arc::new(AtomicUsize::new(0)),
            highlight_done: Arc::new(AtomicBool::new(true)),
            highlight_domain: None,
            _severe_handle,
            _severe_cancel: severe_cancel,
            severe_scanned,
            severe_done,
            parse_lru: Mutex::new(LruCache::new(
                NonZeroUsize::new(FILE_PARSE_LRU).expect("non-zero"),
            )),
            severe_cache,
            vocab_started: AtomicBool::new(false),
        })
    }

    /// Synchronous full index (tests / B-gate smoke).
    pub fn open_sync(path: impl Into<PathBuf>) -> io::Result<Self> {
        let path = path.into();
        let file = File::open(&path)?;
        let mmap = unsafe { Mmap::map(&file)? };
        let file_bytes = mmap.len();
        let spans = scan_line_spans(&mmap);
        let line_count = spans.len();
        let (tx, rx) = mpsc::channel();
        let _ = tx.send(FileEvent::IndexDone { line_count });

        let mmap = Arc::new(mmap);
        let lines = Arc::new(RwLock::new(spans));
        let index_done = Arc::new(AtomicBool::new(true));
        let severe_cache = Arc::new(RwLock::new(vec![None; line_count]));
        let severe_cancel = Arc::new(AtomicBool::new(false));
        let severe_scanned = Arc::new(AtomicUsize::new(0));
        let severe_done = Arc::new(AtomicBool::new(false));
        // Prefetch severe flags even for sync-open (tests + find_severe path).
        let _severe_handle = Some(scan::spawn_severe_prefetch(
            Arc::clone(&mmap),
            Arc::clone(&lines),
            Arc::clone(&index_done),
            Arc::clone(&severe_cancel),
            Arc::clone(&severe_cache),
            Arc::clone(&severe_scanned),
            Arc::clone(&severe_done),
        ));

        Ok(Self {
            path,
            mmap,
            lines,
            file_bytes,
            indexed_bytes: Arc::new(AtomicUsize::new(file_bytes)),
            index_done,
            event_tx: tx,
            events: rx,
            _index_handle: None,
            filter_handle: None,
            filter_cancel: Arc::new(AtomicBool::new(false)),
            filter_gen: Arc::new(AtomicU64::new(0)),
            filter_scanned: Arc::new(AtomicUsize::new(0)),
            filter_done: Arc::new(AtomicBool::new(true)),
            highlight_handle: None,
            highlight_cancel: Arc::new(AtomicBool::new(false)),
            highlight_gen: Arc::new(AtomicU64::new(0)),
            highlight_scanned: Arc::new(AtomicUsize::new(0)),
            highlight_done: Arc::new(AtomicBool::new(true)),
            highlight_domain: None,
            _severe_handle,
            _severe_cancel: severe_cancel,
            severe_scanned,
            severe_done,
            parse_lru: Mutex::new(LruCache::new(
                NonZeroUsize::new(FILE_PARSE_LRU).expect("non-zero"),
            )),
            severe_cache,
            vocab_started: AtomicBool::new(false),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn line_count(&self) -> usize {
        self.lines.read().expect("lines lock").len()
    }

    pub fn index_done(&self) -> bool {
        self.index_done.load(Ordering::Acquire)
    }

    pub fn event_sender(&self) -> Sender<FileEvent> {
        self.event_tx.clone()
    }

    pub fn progress(&self) -> FileProgress {
        FileProgress {
            indexed_lines: self.line_count(),
            indexed_bytes: self.indexed_bytes.load(Ordering::Relaxed),
            file_bytes: self.file_bytes,
            index_done: self.index_done(),
            filter_scanned: self.filter_scanned.load(Ordering::Relaxed),
            filter_done: self.filter_done.load(Ordering::Acquire),
            filter_gen: self.filter_gen.load(Ordering::Relaxed),
            highlight_scanned: self.highlight_scanned.load(Ordering::Relaxed),
            highlight_done: self.highlight_done.load(Ordering::Acquire),
            highlight_gen: self.highlight_gen.load(Ordering::Relaxed),
            severe_scanned: self.severe_scanned.load(Ordering::Relaxed),
            severe_done: self.severe_done.load(Ordering::Acquire),
        }
    }

    /// Drain pending worker events (non-blocking).
    pub fn drain_events(&self) -> Vec<FileEvent> {
        let mut out = Vec::new();
        loop {
            match self.events.try_recv() {
                Ok(ev) => out.push(ev),
                Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => break,
            }
        }
        out
    }

    /// Cancel any in-flight filter and start a new scan with `pred`.
    /// Returns the new generation id.
    ///
    /// Previous workers are signalled via their own cancel flag and detached
    /// (no UI-thread `join`). Each scan gets fresh `filter_scanned` /
    /// `filter_done` arcs so a racing old worker cannot mark the new scan done.
    pub fn start_filter_scan(&mut self, pred: FilterPred) -> u64 {
        self.filter_cancel.store(true, Ordering::Release);
        drop(self.filter_handle.take());
        self.filter_cancel = Arc::new(AtomicBool::new(false));
        self.filter_scanned = Arc::new(AtomicUsize::new(0));
        self.filter_done = Arc::new(AtomicBool::new(false));
        let gen = {
            let g = self.filter_gen.load(Ordering::Relaxed) + 1;
            self.filter_gen.store(g, Ordering::Release);
            g
        };

        let handle = spawn_filter_scan(
            Arc::clone(&self.mmap),
            Arc::clone(&self.lines),
            Arc::clone(&self.index_done),
            Arc::clone(&self.filter_cancel),
            Arc::clone(&self.filter_scanned),
            Arc::clone(&self.filter_done),
            gen,
            pred,
            self.event_tx.clone(),
        );
        self.filter_handle = Some(handle);
        gen
    }

    /// Cancel in-flight filter without starting a new one (filter inactive).
    pub fn cancel_filter_scan(&mut self) {
        self.filter_cancel.store(true, Ordering::Release);
        drop(self.filter_handle.take());
        self.filter_done = Arc::new(AtomicBool::new(true));
        self.filter_scanned = Arc::new(AtomicUsize::new(0));
    }

    /// Cancel any in-flight highlight scan and start a new Vis-domain scan.
    /// Returns the generation id.
    pub fn start_highlight_scan(
        &mut self,
        domain: Arc<HighlightDomain>,
        pattern: &regex::Regex,
    ) -> u64 {
        self.highlight_cancel.store(true, Ordering::Release);
        drop(self.highlight_handle.take());
        self.highlight_cancel = Arc::new(AtomicBool::new(false));
        self.highlight_scanned = Arc::new(AtomicUsize::new(0));
        self.highlight_done = Arc::new(AtomicBool::new(false));
        let gen = {
            let g = self.highlight_gen.load(Ordering::Relaxed) + 1;
            self.highlight_gen.store(g, Ordering::Release);
            g
        };
        self.highlight_domain = Some(Arc::clone(&domain));
        let handle = scan::spawn_highlight_scan(
            Arc::clone(&self.mmap),
            Arc::clone(&self.lines),
            domain,
            Arc::clone(&self.highlight_cancel),
            Arc::clone(&self.highlight_scanned),
            Arc::clone(&self.highlight_done),
            gen,
            pattern.clone(),
            self.event_tx.clone(),
        );
        self.highlight_handle = Some(handle);
        gen
    }

    /// Cancel highlight scan without starting a new one.
    pub fn cancel_highlight_scan(&mut self) {
        self.highlight_cancel.store(true, Ordering::Release);
        drop(self.highlight_handle.take());
        self.highlight_done = Arc::new(AtomicBool::new(true));
        self.highlight_scanned = Arc::new(AtomicUsize::new(0));
        self.highlight_domain = None;
    }

    /// Shared domain for Inc growth (FilterBatch / IndexProgress), if scanning.
    pub fn highlight_domain(&self) -> Option<&Arc<HighlightDomain>> {
        self.highlight_domain.as_ref()
    }

    /// Cached severe flag without parsing (`None` = unknown).
    pub fn severe_cached(&self, i: usize) -> Option<bool> {
        let cache = self.severe_cache.read().expect("severe cache");
        cache.get(i).copied().flatten()
    }

    /// Lazy parse line `i` (0-based). Unparseable → raw-fallback EntryRow.
    pub fn row_at(&self, i: usize) -> Option<EntryRow> {
        {
            let mut lru = self.parse_lru.lock().expect("parse lru");
            if let Some(row) = lru.get(&i) {
                return Some(row.clone());
            }
        }
        let span = {
            let lines = self.lines.read().expect("lines lock");
            *lines.get(i)?
        };
        let row = self.parse_span(i, span);
        {
            let mut lru = self.parse_lru.lock().expect("parse lru");
            lru.put(i, row.clone());
        }
        Some(row)
    }

    fn parse_span(&self, i: usize, span: LineSpan) -> EntryRow {
        let start = span.start as usize;
        let end = start.saturating_add(span.len as usize).min(self.mmap.len());
        let bytes = &self.mmap[start..end];
        let cow = String::from_utf8_lossy(bytes);
        let mut row = EntryRow::from_line_or_raw(cow.as_ref());
        row.row_id = (i as u64).saturating_add(1);
        row.severe = self.severe_for(i, &row);
        row
    }

    fn severe_for(&self, i: usize, row: &EntryRow) -> bool {
        {
            let cache = self.severe_cache.read().expect("severe cache");
            if let Some(Some(v)) = cache.get(i) {
                return *v;
            }
        }
        let v = is_severe_row(row);
        let mut cache = self.severe_cache.write().expect("severe cache");
        if cache.len() <= i {
            cache.resize(i + 1, None);
        }
        cache[i] = Some(v);
        v
    }

    /// Ensure severe cache length covers current line count.
    pub fn grow_severe_cache(&self) {
        let n = self.line_count();
        let mut cache = self.severe_cache.write().expect("severe cache");
        if cache.len() < n {
            cache.resize(n, None);
        }
    }

    /// Synchronous filter scan over currently indexed lines (B-gate / tests).
    pub fn scan_filter_sync(&self, mut pred: impl FnMut(&EntryRow) -> bool) -> Vec<usize> {
        let n = self.line_count();
        let mut hits = Vec::new();
        for i in 0..n {
            if let Some(row) = self.row_at(i) {
                if pred(&row) {
                    hits.push(i);
                }
            }
        }
        hits
    }

    /// Sampled vocab feed over currently indexed lines (≤ [`VOCAB_MAX_SAMPLES`]).
    pub fn feed_vocab_sample(&self, mut feed: impl FnMut(&str, &str, &[String])) {
        let n = self.line_count();
        if n == 0 {
            return;
        }
        let stride = n.div_ceil(VOCAB_MAX_SAMPLES).max(1);
        let mut i = 0usize;
        while i < n {
            if let Some(row) = self.row_at(i) {
                let tokens = crate::input::tokenize_msg_for_vocab(&row.msg);
                feed(&row.tag, &row.pkg, &tokens);
            }
            i = i.saturating_add(stride);
        }
        if n > 1 {
            if let Some(row) = self.row_at(n - 1) {
                let tokens = crate::input::tokenize_msg_for_vocab(&row.msg);
                feed(&row.tag, &row.pkg, &tokens);
            }
        }
    }

    pub fn mark_vocab_started(&self) -> bool {
        !self.vocab_started.swap(true, Ordering::AcqRel)
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

fn scan_line_spans(mmap: &[u8]) -> Vec<LineSpan> {
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

fn index_file_background(
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
fn spawn_filter_scan(
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
        let mut batch = Vec::with_capacity(FILTER_BATCH_HITS);
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
                if pred(&row) {
                    batch.push(i);
                    if batch.len() >= FILTER_BATCH_HITS {
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
                    }
                }
                i += 1;
                filter_scanned.store(i, Ordering::Relaxed);
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_temp(contents: &str) -> tempfile::NamedTempFile {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(contents.as_bytes()).unwrap();
        f.flush().unwrap();
        f
    }

    #[test]
    fn scan_empty_file() {
        let f = write_temp("");
        let store = FileStore::open_sync(f.path()).unwrap();
        assert_eq!(store.line_count(), 0);
        assert!(store.index_done());
    }

    #[test]
    fn scan_no_trailing_newline() {
        let f = write_temp(
            "04-02 10:00:00.000  1  1 I TagA   : a\n04-02 10:00:01.000  1  1 E TagB   : b",
        );
        let store = FileStore::open_sync(f.path()).unwrap();
        assert_eq!(store.line_count(), 2);
        assert_eq!(store.row_at(0).unwrap().tag, "TagA");
        assert_eq!(store.row_at(1).unwrap().tag, "TagB");
    }

    #[test]
    fn unparseable_line_raw_fallback() {
        let f = write_temp("not a log line\n04-02 10:00:00.000  1  1 I TagA   : ok\n");
        let store = FileStore::open_sync(f.path()).unwrap();
        assert_eq!(store.line_count(), 2);
        let raw = store.row_at(0).unwrap();
        assert_eq!(raw.raw, "not a log line");
        assert!(raw.tag.is_empty());
        assert!(!raw.is_parsed());
        assert_eq!(raw.row_id, 1);
        assert!(store.row_at(1).unwrap().is_parsed());
        assert_eq!(store.row_at(1).unwrap().tag, "TagA");
    }

    #[test]
    fn filter_sync_subset() {
        let f = write_temp(
            "04-02 10:00:00.000  1  1 I TagA   : a\n\
             04-02 10:00:01.000  1  1 E TagB   : b\n\
             04-02 10:00:02.000  1  1 I TagA   : c\n",
        );
        let store = FileStore::open_sync(f.path()).unwrap();
        let hits = store.scan_filter_sync(|r| r.tag == "TagA");
        assert_eq!(hits, vec![0, 2]);
    }

    #[test]
    fn row_ref_deref() {
        let row = EntryRow::from_line("04-02 10:00:00.000  1  1 I TagA   : m").unwrap();
        let r = RowRef::Owned(row);
        assert_eq!(r.tag, "TagA");
    }

    #[test]
    fn non_utf8_lossy() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(b"04-02 10:00:00.000  1  1 I TagA   : ")
            .unwrap();
        f.write_all(&[0xFF, 0xFE]).unwrap();
        f.write_all(b"\n").unwrap();
        f.flush().unwrap();
        let store = FileStore::open_sync(f.path()).unwrap();
        let row = store.row_at(0).unwrap();
        assert!(!row.raw.is_empty());
    }

    #[test]
    fn bg_index_completes() {
        let f = write_temp(
            "04-02 10:00:00.000  1  1 I TagA   : a\n04-02 10:00:01.000  1  1 E TagB   : b\n",
        );
        let store = FileStore::open(f.path()).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while !store.index_done() {
            if std::time::Instant::now() > deadline {
                panic!("index timed out");
            }
            let _ = store.drain_events();
            thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(store.line_count(), 2);
    }

    #[test]
    fn vocab_sample_capped() {
        let mut body = String::new();
        for i in 0..20_000 {
            body.push_str(&format!("04-02 10:00:00.000  1  1 I Tag{i}   : line{i}\n"));
        }
        let f = write_temp(&body);
        let store = FileStore::open_sync(f.path()).unwrap();
        let mut feeds = 0usize;
        store.feed_vocab_sample(|_, _, _| feeds += 1);
        // stride + optional last line; must stay near VOCAB_MAX_SAMPLES.
        assert!(feeds <= VOCAB_MAX_SAMPLES + 1, "feeds={feeds}");
        assert!(feeds >= VOCAB_MAX_SAMPLES / 2, "feeds={feeds}");
    }
}
