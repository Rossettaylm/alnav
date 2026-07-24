# Design: TUI --hdc Visible::All + drop-oldest ingest ring

## Goals / Non-goals

**Goals**: Stream/`--hdc` path — structural `Visible::All`, O(1) eviction, bounded drop-oldest ingest ring (P-after). Preserve filter/matched/following/bookmark semantics.

**Non-goals**: mmap file backend, P-late parse, blocking backpressure, drop-count status badge (optional later).

## Architecture

```
hilog ──► hdc thread: read → EntryRow::from_line → RingBuffer<EntryRow, CAP>
                                                      │ full ⇒ pop_front (drop oldest)
main:     drain(ring) → push_row → rows / matched
                                      │
                         visible: All { len }   // identity into view_source
```

## `Visible` model

```rust
enum Visible {
    All { len: usize },           // identity 0..len into view_source
    // Subset reserved for file task; may be omitted until mmap lands:
    // Subset(Vec<u32>),
}
```

| State | `view_source` | `visible` |
|-------|---------------|-----------|
| filter inactive | `rows` | `All { len: rows.len() }` |
| filter active | `matched` | `All { len: matched.len() }` |

Read paths that today do `visible[i]` → `view_source()[visible[i]]` become `view_source()[i]` when `All` (cursor indexes the source directly, or indexes `0..len` identically).

### Eviction (replaces `shift_visible_after_front_evict` for Stream)

When `!active` and `rows` at `max_lines`: `pop_front` + `push_back`; `All.len` unchanged; O(1) adjust `cursor` / `list_offset` / `visual_anchor` if the evicted front was on-screen (same intent as today when `visible[0]==0`).

When `matched` hits `matched_cap`: same O(1) adjust; no O(n) index walk.

`rebuild_visible`: set `All { len: view_source.len() }` — do **not** allocate `0..n`.

## Ingest ring (P-after)

- Custom bounded queue shared by producer and main (e.g. `Mutex<VecDeque<EntryRow>>` + `Condvar`/`try`, or equivalent). **Not** `sync_channel` alone (cannot drop-oldest on full).
- CAP: start **8192** (tunable const); document in code.
- Producer: parse → if ring len == CAP { pop_front }; push_back. Never block on full.
- `App::drain`: try-lock / non-blocking pop until empty or per-frame budget (budget optional MVP; recommend soft cap e.g. 4096/frame to protect UI if ring was full of bursts).
- `spawn_file_ingest` may keep unbounded or share the same ring type for consistency until mmap removes it; must not reintroduce O(n²) via `push_row`.

## Compatibility

- `-f` until mmap: still owned rows; benefits from `Visible::All` automatically if it shares `push_row`.
- File sparse `Subset` arrives in mmap task; do not block this task on `RowStore`/`RowRef`.

## Risks

- **Drop under flood**: intentional; session never sees dropped undrained lines. Accept.
- **Call-site churn**: every `visible[i]` reader must understand `All` vs future `Subset`. Prefer a single helper `fn visible_source_idx(&self, vis_i: usize) -> Option<usize>` / `fn visible_len`.
- **Tests**: many construct identity `visible` vecs — update to `All` or go through helpers.

## Files to touch

| File | Change |
|------|--------|
| `app.rs` | `Visible` enum; migrate readers; O(1) eviction; `rebuild_visible`; `drain` from ring |
| `ingest.rs` | hdc → bounded drop-oldest ring; return handle/`Receiver`-like API |
| `main.rs` | wire ring drain for `--hdc` |
| tests in `app.rs` / `ingest.rs` / `main` | identity visible + backpressure |

## Relation to mmap task

Sibling `07-24-tui-mmap-file-backend` adds `RowStore`/`RowRef`/`FileStore` and `Visible::Subset`. **This task lands first** so Stream is already `All` when File arrives.
