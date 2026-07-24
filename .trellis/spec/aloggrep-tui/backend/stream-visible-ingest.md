# Stream Visible + HDC Ingest Ring

> Executable contracts for `Visible::All` and `--hdc` drop-oldest ingest (task `07-24-tui-hdc-stream-visible`).

---

## Scenario: Identity visible set (`Visible::All`)

### 1. Scope / Trigger

- Trigger: any owned `VecDeque` Stream path (`--hdc`, and `-f` until mmap).
- Cross-module: `app::Visible`, `App::push_row` / `rebuild_visible` / `drain`, ui/preview readers via `source_idx_for_visible`.

### 2. Signatures

```rust
// app.rs
pub enum Visible {
    All { len: usize },
    Subset(Vec<usize>), // file/mmap filter hits (line indices into FileStore)
}
impl Visible {
    pub fn len(&self) -> usize;
    pub fn source_idx(&self, vis_i: usize) -> Option<usize>; // All ⇒ Some(i) if i < len
}

pub fn visible_len(&self) -> usize;
pub fn source_idx_for_visible(&self, vis_i: usize) -> Option<usize>;
```

### 3. Contracts

| State | `view_source` | `visible` |
|-------|---------------|-----------|
| filter inactive | `rows` | `All { len: rows.len() }` |
| filter active | `matched` | `All { len: matched.len() }` |

- Never materialise identity as `Vec<usize>` of `0..n`.
- Front eviction (`max_lines` / `matched_cap`): only after real `pop_front().is_some()`; adjust `cursor` / `list_offset` / `visual_anchor` in **O(1)**; `All.len` unchanged across pop+push.
- Empty `visible` ⇒ no on-screen adjust (evicted front was not visible).
- Readers: `source_idx_for_visible(vis_i)` then index `view_source`; do not assume `visible: Vec`.

### 4. Validation & Error Matrix

| Case | Expected |
|------|----------|
| inactive, rows at `max_lines`, push | O(1) adjust; no full-table shift |
| active, matched at cap, push match | O(1) adjust on matched front evict |
| `max_lines == 0` / empty pop | no adjust |
| `rebuild_visible` | set `All { len }` only |

### 5. Good / Base / Bad

- **Good**: inactive flood past `max_lines` stays interactive; cursor/offset coherent.
- **Base**: filter on → `matched` identity `All`; rows churn does not shift visible.
- **Bad**: restoring `Vec<usize>` identity + per-row `-= 1` (O(n²) regression).

### 6. Tests Required

- inactive front evict decrements `list_offset` without scanning a vec
- drain budget leaves remainder / defers `ingest_done` until ring empty after disconnect
- existing matched-survive / following / bookmark regressions stay green

### 7. Wrong vs Correct

| Wrong | Correct |
|-------|---------|
| `visible.push(rows.len()-1)` + shift all on evict | `visible = All { len: rows.len() }` + O(1) adjust |
| `for i in visible.iter_mut() { *i -= 1 }` | never for Stream identity |

---

## Scenario: HDC P-after drop-oldest ring

### 1. Scope / Trigger

- Trigger: `--hdc` live ingest backpressure.
- Cross-module: `ingest::{DropOldestRing, IngestHandle, TryRecvRow, INGEST_RING_CAP}`, `App::drain`, `main` event loop.

### 2. Signatures

```rust
// ingest.rs
pub const INGEST_RING_CAP: usize = 8192;

pub enum TryRecvKind { Empty, Disconnected }
pub trait TryRecvRow {
    fn try_recv_row(&self) -> Result<EntryRow, TryRecvKind>;
}
pub enum IngestHandle {
    Channel(Receiver<EntryRow>), // legacy/tests; production `-f` uses FileStore
    Ring(Arc<DropOldestRing>),   // hdc
}
pub struct DropOldestRing { /* ... */ }
impl DropOldestRing {
    pub fn new(cap: usize) -> Arc<Self>;
    pub fn push(&self, row: EntryRow); // never blocks; drop oldest if full
    pub fn try_pop(&self) -> Result<EntryRow, TryRecvKind>;
    pub fn disconnect(&self);
}

// app.rs
const DRAIN_BUDGET_PER_FRAME: usize = 4096;
pub fn drain(&mut self, ingest: &impl TryRecvRow);
```

### 3. Contracts

- **P-after**: producer thread still `EntryRow::from_line`, then `ring.push`.
- Full ring: `pop_front` oldest undrained row, then push — **never** block `hilog`.
- `Disconnected` only when producer finished **and** ring empty (partial drain must not set `ingest_done` early).
- `drain` stops at Empty, Disconnected, or `DRAIN_BUDGET_PER_FRAME`.
- Dropped undrained rows are intentional; no status badge required.

### 4. Validation & Error Matrix

| Case | Expected |
|------|----------|
| push when len == CAP | drop oldest, accept newest |
| producer ends, ring still has rows | `try_pop` Ok until empty, then Disconnected |
| one frame > budget | leave remainder; next frame continues |

### 5. Good / Base / Bad

- **Good**: UI lag under flood; newest lines still arrive; hilog thread not blocked.
- **Base**: production `-f` uses `FileStore` (mmap); `IngestHandle::Channel` remains for tests.
- **Bad**: `sync_channel` that blocks producer, or `try_send` drop-newest.

### 6. Tests Required

- ring drops oldest when over capacity
- drain budget defers `ingest_done`

### 7. Wrong vs Correct

| Wrong | Correct |
|-------|---------|
| unbounded `mpsc` for hdc | `DropOldestRing` + `IngestHandle::Ring` |
| block on full | drop oldest undrained |
| parse on UI thread for hdc (P-late) | P-after on producer (until a future task) |
