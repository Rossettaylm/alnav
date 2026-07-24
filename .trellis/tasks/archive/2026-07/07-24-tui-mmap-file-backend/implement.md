# Implement: TUI -f mmap file backend

**Start only after** `07-24-tui-hdc-stream-visible` is done (`Visible::All` on Stream).

Each phase ends green (`cargo test -p aloggrep-tui`).

## Phase A — `RowStore` / `RowRef` (Stream wrap, behavior identical)

1. Add `RowRef` + `RowStore::Stream(StreamStore)` wrapping post-hdc `rows`/`matched`/`Visible::All`.
2. Migrate all read sites to `store.row_at`.
3. Green gate: no file behavior change yet.

## Phase B — FileStore sync (milestone / B-gate)

1. Cargo: `memmap2`, `memchr`.
2. `FileStore`: mmap + sync newline index + lazy `row_at` + LRU + raw-fallback.
3. File: `visible = All` or `Subset` from **sync** filter scan; no `max_lines` eviction.
4. `main`: `-f` → `FileStore` (drops file row channel).
5. Smoke medium/large file: full browse, mem down. **Not** final acceptance if filter freezes.

## Phase C — Backgrounding (acceptance gate)

1. Incremental indexer + progress; interactive during index.
2. Cancellable filter scan + incremental `Subset` batches + status %.
3. Sampled vocab + lazy severe cache.
4. Manual: 700MB opens in seconds; filter does not freeze UI.

## Validation

```bash
cargo test -p aloggrep-tui store::
cargo test -p aloggrep-tui app::
cargo test -p aloggrep-tui ui::
cargo test --workspace
cargo fmt -p aloggrep-tui --check
```

Manual (TTY): 700MB sample path in PRD; filter/highlight/search/minimap/bookmark/`yc`; `--hdc` smoke (no regression vs hdc task).

## Before `task.py start`

- [x] Split from combined plan; hdc sibling owns Stream Visible/ring
- [x] User approved B-gate + hdc→file order
- [x] `prd.md` / `design.md` / `implement.md` present
- [ ] hdc task completed (hard prerequisite)
- [ ] `implement.jsonl` / `check.jsonl` curated for FileStore scope
- [ ] Review gate with user, then `task.py start`
