# Design: TUI -f mmap file backend

## Goals / Non-goals

**Goals**: for read-only `-f` files, decouple startup time and memory from file
size via mmap + line index + lazy parse; remove the 500k `max_lines` cap for
files (full-file browse); introduce `RowStore` / `RowRef` with `FileStore` +
existing Stream; filter hits as `Visible::Subset` line numbers.

**Non-goals**: live file tail, multi-file glob, on-disk date index, stdin pipe,
`--hdc` Visible/backpressure work (sibling task `07-24-tui-hdc-stream-visible`).

## Prerequisite

`Visible::All` and Stream O(1) eviction already on tree from the hdc task.
This design **extends** `Visible` with `Subset` and adds `RowStore`.

## Architecture

```
                         ┌──────────────────────────────────────────┐
  -f file  ──mmap──►     │ FileStore                                 │
                         │  mmap: Arc<Mmap>                          │
                         │  lines: Vec<LineSpan{start:u64,len:u32}>  │  (bg-built)
                         │  parse(i)->EntryRow (lazy, small LRU)     │
                         │  severe_cache: Vec<Sev> (lazy)            │
                         └──────────────────────────────────────────┘
  --hdc    ──ring────►   ┌──────────────────────────────────────────┐
                         │ StreamStore (from hdc task)               │
                         │  rows/matched: VecDeque<EntryRow>         │
                         │  visible: All { len }                     │
                         └──────────────────────────────────────────┘
                                        │
              App.store: RowStore { File(..) | Stream(..) }
                                        │
         visible: All | Subset(line nos into file index / identity into stream)
                                        │
     row_at(vis_idx) -> RowRef<'a> : Deref<Target = EntryRow>
       Stream => Borrowed(&buf[i])   File => Owned(parse(lines[i]))
```

## Core abstraction: `RowStore` + `RowRef`

```rust
pub enum RowRef<'a> {
    Borrowed(&'a EntryRow),
    Owned(EntryRow),
}
impl Deref for RowRef<'_> { type Target = EntryRow; ... }

pub enum RowStore { File(FileStore), Stream(StreamStore) }
```

Migrate every reader of `view_source()[…]` to `store.row_at(…)`.

## `visible` semantics

| Store / state | `visible` | filtered subset |
|---|---|---|
| Stream, any | `All { len }` | `matched` owns rows when filter on |
| File, filter inactive | `All { len: line_count }` | — |
| File, filter active | `Subset(hit line numbers)` | no owned `matched` |

`Subset` at 4M hits ≈ 16–32MB — acceptable MVP. Optional later: avoid materializing identity `All` as a vec (already true if `All` is len-only).

## Line indexing / lazy parse / background scans

Unchanged from prior combined design:

- `memmap2::Mmap` + `memchr` newline scan; `LineSpan { start:u64, len:u32 }`.
- Incremental indexer + cancellable filter scanner (`generation`) + sampled vocab + lazy severe cache.
- Unparseable file lines → raw-fallback `EntryRow` (behavior delta vs stream; tested).

## B-gate rollout

| Phase | Ship? | Notes |
|-------|-------|-------|
| B sync index + sync filter | milestone only | may briefly block on open/filter |
| C background index/filter | **acceptance** | interactive load/filter |

## Memory budget (700MB file)

| Item | mmap backend |
|---|---|
| File data | OS-paged mmap |
| Line index | ~48MB (`LineSpan` × 4M) |
| Filtered set | `Subset` ≤ ~32MB |
| severe cache | ~4MB lazy |
| Parsed rows | ~256-row LRU |

## Risks & mitigations

- mmap SIGBUS if file truncated under mapping — accepted; document.
- Read-path breadth — `RowRef: Deref`; Phase A Stream-only wrap after hdc task.
- Non-UTF-8 — `from_utf8_lossy`.

## Files to touch

| File | Change |
|------|--------|
| `aloggrep-tui/Cargo.toml` | `memmap2`, `memchr` |
| `store.rs` (**new**) | `RowStore`/`FileStore`/`StreamStore`/`RowRef` |
| `model.rs` | raw-fallback ctor for unparseable file lines |
| `app.rs` | hold `store`; File `rebuild_visible` = scan → `Subset`; bg in Phase C |
| `ingest.rs` | `-f` → mmap open/index spawner (no row channel) |
| `main.rs` | `-f` builds `FileStore`; progress drain |
| `ui.rs` / `preview.rs` | `row_at`; loading progress |
| `CLAUDE.md` + `.trellis/spec/aloggrep-tui/backend/*` | document FileStore |

## Relation to hdc task

Do **not** re-implement drop-oldest ring or `Visible::All` here — wrap Stream into `RowStore` and add `Subset` + `FileStore` only.
