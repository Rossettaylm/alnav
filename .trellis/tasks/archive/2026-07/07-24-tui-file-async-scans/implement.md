# Implement: TUI file async scans + LogList loading

Each phase ends green: `cargo test -p aloggrep-tui`.

## Phase 1 — Audit + Filter reinforcement

1. Grep File-mode paths for `0..visible.len()` + `row_at` / `compute_match_stats_inner`.
2. Ensure `FilterBatch`/`FilterDone` never trigger UI full parse; restart highlight job instead.
3. Green gate.

## Phase 2 — Highlight Vis+Inc worker

1. Add highlight scan state + worker (events or channel) over visible source snapshot.
2. Inc on visible growth; cancel on gen bump.
3. Wire `highlight_match_stats` / `n`/`N` / `find_match` to hit index for File.
4. Tests: incremental hits, cancel on filter change, stats without UI parse.

## Phase 3 — Severe

1. Background or index-time severe_cache fill; `find_severe` reads cache.
2. Test: no full visible parse on find_severe for File.

## Phase 4 — LogList loading UI

1. `log_loading_label` for index/filter/highlight.
2. Render in LogList title/top via theme.
3. Free: existing keys still work under loading (unit or manual).

## Validation

```bash
cargo test -p aloggrep-tui
cargo fmt -p aloggrep-tui --check
```

Manual TTY:
```bash
aloggrep-tui -f /Users/lyman/Downloads/extracQQXLog_HarmonyQQ_2026.07.22.10-2026.07.24.10/merged_2026.07.22-24.log
```
Add filter + highlight; confirm LogList loading text; `j/k`/`n`/`N` stay responsive.

## Before `task.py start`

- [x] Grilling decisions locked; mmap committed first (`0dca14d`)
- [x] `prd.md` / `design.md` / `implement.md` present
- [ ] `implement.jsonl` / `check.jsonl` curated
- [ ] User review gate, then `task.py start`
