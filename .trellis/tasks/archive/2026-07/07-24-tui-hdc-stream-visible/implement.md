# Implement: TUI --hdc Visible::All + drop-oldest ingest ring

Each step ends green: `cargo test -p aloggrep-tui`.

## Phase 1 — `Visible::All` (behavior-identical, no ring yet)

1. Introduce `Visible::{ All { len } }` (Subset stub optional).
2. Replace `App.visible: Vec<usize>` usages for Stream identity cases with `All`.
3. Add helpers: `visible_len`, `source_idx_for_visible`, cursor still `0..visible_len`.
4. Rewrite `!active` / `matched` front-evict paths to O(1); delete or bypass full-table `shift_visible_after_front_evict` for identity cases.
5. `rebuild_visible` → `All { len }` only.
6. Green gate: existing app/ui tests updated; no hdc flood required yet.

## Phase 2 — P-after drop-oldest ring

1. `ingest.rs`: ring type + `spawn_hdc_ingest` writes through ring (CAP=8192).
2. `App::drain` / main loop consume ring (non-blocking); optional per-frame budget.
3. Tests: ring drops oldest when full; consumer sees newest; disconnect/clear still work.
4. Manual `--hdc` smoke: sustained flood stays interactive; no O(n²) stall after `max_lines`.

## Validation

```bash
cargo test -p aloggrep-tui app::
cargo test -p aloggrep-tui ingest::
cargo test -p aloggrep-tui
cargo fmt -p aloggrep-tui --check
```

Manual (TTY): `aloggrep-tui --hdc` under heavy device log rate — scroll/follow remains usable; RSS stable vs unbounded channel era.

## Before `task.py start`

- [x] User grilled + approved S1 / P-after / drop-oldest / hdc-first split
- [x] `prd.md` / `design.md` / `implement.md` present
- [ ] `implement.jsonl` / `check.jsonl` curated
- [ ] Review gate with user, then `task.py start`

## Ordering

Complete and archive (or clearly done) **before** starting `07-24-tui-mmap-file-backend`.
