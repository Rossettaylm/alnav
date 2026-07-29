# File Async Scans (Highlight / Severe / Loading)

> Executable contracts for task `07-24-tui-file-async-scans` (All-scan A1).

---

## Scenario: Highlight Vis + Inc

### Contracts

| Topic | Rule |
|-------|------|
| Domain | Current `visible` (`All` identity or `Subset` line indices) |
| Worker | Parses on background thread via mmap + `LineSpan`; UI never O(visible) `row_at` for stats/`n`/`N` |
| Inc | FilterBatch / IndexProgress grow shared `HighlightDomain`; worker continues from `scanned_vis` |
| Invalidate | Filter rebuild, active highlight change, clear/delete → bump gen, clear hits, restart |
| Stats / nN | `HighlightScanState` hit index (binary search); Stream keeps sync scan |
| Wrap | `n`/`N` **no wrapscan** (boundary → `NO MORE`); while Inc, never jump past known hits |
| Minimap | File paints severe from `severe_cache` + highlight from hit index — no per-frame `row_at` |

## Scenario: Severe prefetch

| Topic | Rule |
|-------|------|
| Prefetch | Started with `FileStore::open` / `open_sync`; fills `severe_cache` |
| find_severe | Prefer `severe_cached`; on miss, budget-capped sync parse (`SEVERE_SYNC_PARSE_BUDGET`) |
| Minimap | File samples `severe_cached` only (never UI `row_at` for marks) |

## Scenario: LogList loading (L1 + T + Free)

| Topic | Rule |
|-------|------|
| Jobs | index / filter / highlight |
| UI | Title suffix via `theme::numbered_title_with_loading` + `log_loading_style` |
| Free | No blocking overlay; j/k and keys remain live |

## Validation

```bash
cargo test -p alnav
cargo fmt -p alnav --check
```
