# Design: TUI file async scans + LogList loading

## Goals / Non-goals

**Goals**: A1 background scans for File mode — reinforce filter, Vis+Inc highlight hit index (`n`/`N`), severe without UI full parse; LogList L1+T+Free loading.

**Non-goals**: mmap redo, vocab/ts full index, blocking overlay, hdc ring changes.

## Architecture

```
FileStore (mmap + lines)
        │
        ├── IndexWorker (existing) ──► IndexProgress/Done
        ├── FilterWorker (existing) ──► FilterBatch/Done → Visible::Subset
        └── ScanHub (new or extend FileStore events)
                ├── HighlightWorker (Vis domain, Inc cursor)
                └── SeverePrefetch (optional; or extend severe_cache fill)
                        │
App.poll_* ── merge hits / progress ──► UI (no O(n) row_at)
LogList title/banner ← file_loading_label() (theme)
```

## Highlight hit index

```rust
struct HighlightScanState {
    gen: u64,
    /// Vis indices that match active highlight group (sorted ascending).
    hits: Vec<usize>,
    /// Next vis slot to scan (Inc).
    scanned_vis: usize,
    done: bool,
}
```

- Input domain: `0..visible.len()` mapped through `source_idx_for_visible` → `FileStore::row_at` **on worker**.
- Snapshot for worker: `Arc` of current source line indices for `visible` (copy `Subset` vec or `0..n` len) + pattern/regex from active highlight group + `gen`.
- On `visible` grow (IndexProgress / FilterBatch): if same gen lineage, raise target len; worker Inc from `scanned_vis`.
- On filter_gen change, highlight edit, clear highlight: bump gen, clear hits, restart.
- `highlight_match_stats`: O(log n) from `hits` + cursor position among hits — **never** `compute_match_stats_inner` full parse on File.
- `find_match` / `n`/`N`: binary search / next in `hits`.

## Filter reinforcement

- Keep `start_filter_scan`; ensure `poll_file_store` never sets paths that call File `recompute_match_stats` with full scan — only mark highlight scan stale / restart Inc.
- Remove or gate any remaining UI `row_at` loops over `visible.len()` for File (audit `find_match`, `jump_first_match_of`, `highlight_match_stats`).

## Severe

- Prefer filling `severe_cache` from worker (piggyback index or dedicated low-priority pass).
- `find_severe`: walk cache / indexed flags only; parse on miss for single line OK, not full visible.

## LogList loading (L1 + T + Free)

- `App::log_loading_label() -> Option<String>` aggregating:
  - `!index_done` → indexing %
  - filter active && !filter_done → filtering %
  - highlight active && !highlight_done → highlighting % (priority: show most relevant or combine short)
- `ui::render_log_list`: put label in block title or inner top line via `theme::plain_title` / dim accent — **no** `Color::*` in ui.rs.
- Input: unchanged; Free navigation.

## Stream

- If `visible.len()` bounded by `max_lines`, sync stats OK; or share hit-index API with immediate complete for Stream.
- Do not regress `Visible::All` / ring.

## Risks

- Worker holding stale Subset snapshot — gen must invalidate.
- Huge hit vectors (millions) — same class as Subset; acceptable MVP; optional `u32` later.
- Theme: add tokens if needed (`loading_banner` style).

## Files to touch

| File | Change |
|------|--------|
| `store.rs` / new `scan.rs` | Highlight/severe workers + events |
| `app.rs` | hit index state; replace File match stats / nN; loading label; poll |
| `ui.rs` | LogList title/banner |
| `theme.rs` | loading text style token |
| `highlight_model.rs` | helpers to clone pattern for worker if needed |
| specs | `file-store.md` + optional `async-scans.md` |

## Relation to mmap

Builds on `FileStore` + `Visible::Subset`; does not replace them.
