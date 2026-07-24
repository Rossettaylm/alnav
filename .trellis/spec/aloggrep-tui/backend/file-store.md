# FileStore (mmap + lazy parse)

> Executable contracts for `-f` mmap backend (task `07-24-tui-mmap-file-backend`).

---

## Scenario: RowStore / FileStore

### 1. Scope / Trigger

- Trigger: `aloggrep-tui -f <path>` read-only huge logs.
- Cross-module: `store::{RowStore,FileStore,StreamStore,RowRef}`, `App`, `main` poll loop, ui/preview via `row_at`.

### 2. Signatures

```rust
// store.rs
pub enum RowRef<'a> { Borrowed(&'a EntryRow), Owned(EntryRow) } // Deref → EntryRow
pub enum RowStore { File(FileStore), Stream(StreamStore) }

impl FileStore {
    pub fn open(path: &Path) -> io::Result<Self>; // mmap + bg index
    pub fn line_count(&self) -> usize;
    pub fn row_at(&self, line_idx: usize) -> Option<EntryRow>; // lazy parse + LRU
    pub fn start_filter_scan(&mut self, pred: FilterPred) -> u64;
    pub fn cancel_filter_scan(&mut self);
    pub fn drain_events(&mut self) -> Vec<FileEvent>;
    pub fn progress(&self) -> FileProgress;
}

pub enum FileEvent {
    IndexProgress { line_count: usize, /* ... */ },
    IndexDone { line_count: usize },
    FilterBatch { gen: u64, hits: Vec<usize>, scanned: usize },
    FilterDone { gen: u64, scanned: usize },
}
```

### 3. Contracts

| Topic | Rule |
|-------|------|
| Memory | Never materialise all lines as owned `EntryRow`; mmap + `LineSpan` index only |
| Cap | File mode: no `max_lines` eviction; full-file browse |
| Visible | inactive → `Visible::All { len }`; filter → `Visible::Subset(line indices)` |
| Filter | Background, cancellable (`gen`); incremental `FilterBatch`; UI must not O(n) `row_at` on each batch |
| Unparseable | `from_line_or_raw` — show raw line (delta vs stream drop) |
| `--max-lines` | hdc/stream only |
| Vocab | Sampled + hard cap after IndexDone; not full-file parse on UI |

### 4. Validation & Error Matrix

| Case | Expected |
|------|----------|
| open missing path | immediate `io::Error` |
| empty / no trailing NL | no panic; line_count correct |
| filter change mid-scan | old gen dropped; new scan |
| FilterBatch on UI | extend Subset only; defer highlight stats |

### 5. Good / Base / Bad

- **Good**: 700MB opens in seconds; RSS ≪ file size; scroll during index.
- **Base**: small file behaviour matches prior owned-buffer UX for filters/bookmarks.
- **Bad**: channel full-file `EntryRow` ingest for `-f`; UI-thread full parse on filter batch.

### 6. Tests Required

- full browse ignores `max_lines`
- Subset filter hits / cancel gen
- raw fallback for unparseable
- vocab sample cap

### 7. Wrong vs Correct

| Wrong | Correct |
|-------|---------|
| `spawn_file_ingest` → owned rows for production `-f` | `FileStore::open` + `poll_file_store` |
| `matched: VecDeque<EntryRow>` for file filter | `Visible::Subset` line numbers |
| per-frame `row_at` over all visible for minimap bookmarks | `visible_idx_for_row_id` / bounded scans |

## Known follow-up (out of this task)

Highlight / `nN` / severe full-visible scans on UI thread — see task `tui-file-async-scans` (All-scan A1).
