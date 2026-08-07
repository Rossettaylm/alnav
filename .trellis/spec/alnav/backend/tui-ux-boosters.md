# TUI UX Boosters (Crash Detail / Summary Panel / Disconnect / Wrap Toggle)

> Executable contracts for task `08-07-tui-ux-boosters` (4 independent LogList/overlay features).

---

## Overview

Four independent, small-surface TUI additions that each reuse an existing
mechanism instead of adding a parallel one: `P` (Pretty) gains a crash
branch, `Leader i` reuses the async-scan + gen-cancel pattern for a
read-only stats snapshot, the status bar reuses `App.ingest_done`, and `w`
reuses the Preview panel's single-line truncation algorithm.

---

## Scenario: Crash/ANR Detail (reuse `P`)

### 1. Scope / Trigger

Update when changing `DetailView::Pretty` rendering, `CrashDetector` usage
in the TUI, or File-mode continuation-line scanning.

### 2. Signatures

| Item | Location | Contract |
|------|----------|----------|
| `crash_context_for_row(app: &App, row: &EntryRow) -> Option<(CrashInfo, bool)>` | `ui.rs` | `bool` = truncated at `CRASH_SCAN_LIMIT` |
| `CRASH_SCAN_LIMIT` | `ui.rs` (`const`, `500`) | Max continuation lines scanned in File mode |
| `render_crash_detail_lines(info: &CrashInfo, truncated: bool, width: usize) -> Vec<Line<'static>>` | `ui.rs` | Pure renderer, no `App` access |
| `detail_content_lines` `DetailView::Pretty` branch | `ui.rs` | `crash_context_for_row` first; `None` falls back to `detail_pretty_lines` (JSON/raw) |

### 3. Contracts

- **No new keybinding, no new `DetailView` variant.** Detection reuses the
  existing `P` (`toggle_detail_pretty`) key and state machine.
- Gate is `CrashDetector::detect()` (signature regex: `FATAL EXCEPTION` /
  `ANR in ` / native signal) on the **cursor row's own msg** — **not**
  `is_severe_row` (which also matches plain E/F-level lines with no crash
  signature). A plain E-level JSON error line must still hit the JSON
  pretty path, not the crash view.
- **File mode** continuation scan walks **physical line index**, not
  `Visible` index: `app.source_idx_for_visible(app.cursor)` →
  `app.store.row_at_source(idx, false)` stepping `+1`. Never use
  `app.row_at(vis_i + 1)` here — continuation lines (unparsed, `parsed ==
  false`) are usually **not** in `visible` when a text filter is active,
  so walking by `Visible` index skips over real file lines and produces
  wrong/short stacks.
- **Stream mode**: no continuation data exists (`EntryRow::from_line` drops
  unparsable lines at ingest). `crash_context_for_row` uses the row's own
  single-line msg only; `stack` is typically empty. The renderer must show
  a "stream mode has no stack" placeholder, not an empty section that
  looks like a bug.
- Cursor must be on the signature line itself. Landing on a stack
  continuation line (no signature match) returns `None` and falls through
  to the existing JSON/raw chain — there is **no** "find nearest crash
  header above" scan.
- View-only: no yank/export path for `CrashInfo`.

### 4. Validation & Error Matrix

| Condition | Behavior |
|-----------|----------|
| Cursor on crash-signature line, File mode | Structured view, `stack` populated (up to `CRASH_SCAN_LIMIT`) |
| Cursor on crash-signature line, Stream mode | Structured view, `stack` empty + placeholder text |
| Cursor on plain E/F line, no crash signature | Falls back to JSON/raw `detail_pretty_lines` unchanged |
| Cursor on stack continuation line (no signature) | `None` → JSON/raw fallback, no panic |
| Continuation run exceeds 500 lines (File mode) | `truncated = true`, renderer appends "…(已截断)" |

### 5. Good/Base/Bad Cases

- **Good**: File mode, cursor on `FATAL EXCEPTION` line with 40 `at ...`
  continuation lines below it → full stack shown.
- **Base**: Stream mode, same headline, no continuation data → type/headline/
  exception shown, stack section shows placeholder.
- **Bad**: Using `app.row_at(cursor_vis_i + 1)` to walk continuation lines
  while a Filter is active — silently truncates or corrupts the stack
  because filtered-out physical lines are skipped in `Visible` space.

### 6. Tests Required

- `ui::crash_detail_tests` — File continuation merge, 500-line truncation,
  Stream single-line degrade, non-crash-signature row returns `None`,
  cursor on bare continuation line returns `None` without panicking.

### 7. Wrong vs Correct

#### Wrong
```rust
// File-mode continuation scan by Visible index — wrong when filtered
let mut i = app.cursor + 1;
while let Some(r) = app.row_at(i) { ... }
```

#### Correct
```rust
// Physical line index, bypasses Visible filtering
let mut idx = app.source_idx_for_visible(app.cursor)?;
loop {
    idx += 1;
    let Some(r) = app.store.row_at_source(idx, false) else { break };
    if r.parsed { break; }
    // merge r into stack...
}
```

---

## Scenario: Summary Panel (`Leader i`)

### 1. Scope / Trigger

Update when changing `SummaryView`, `summary_gen`, the File/Stream
background summary job, `alnav-core::summary::Summary`'s public surface,
or the summary panel's bar-chart rendering.

### 2. Signatures

| Item | Location | Contract |
|------|----------|----------|
| `Summary::into_report(self, matched: usize) -> SummaryOutput` | `alnav-core/src/summary.rs` | New structured (non-JSON) accessor; `to_json` now calls it internally |
| `SummaryOutput` / `TagEntry` / `ErrorEntry` / `TimeRange` | `alnav-core/src/summary.rs` | `pub` (were private) |
| `ActionId::LeaderSummary` | `keymap.rs` | `KeyContext::Leader`, `toml_key: "summary"`, default `i` |
| `App::open_summary_panel()` / `close_summary_panel()` | `app.rs` | Bump `summary_gen`, spawn/drop background job |
| `App::poll_summary_job()` | `app.rs` | Drains result channel; drops messages where `msg.gen != self.summary_gen` |
| `FileStore::scan_snapshot() -> (Arc<Mmap>, Arc<RwLock<Vec<LineSpan>>>)` | `store.rs` | Read-only handles for out-of-`FileStore` background workers |

### 3. Contracts

- **Data scope is always `visible`** (current filtered result; unfiltered ⇒
  whole set). No global-vs-filtered toggle.
- **Static snapshot only** — computed once when the panel opens; does not
  follow later `visible` growth/filter changes. Reopen (`Leader i` again
  after close) to get fresh data.
- **`to_json`'s JSON output must stay byte-for-byte compatible.** `into_report`
  carries all the sort/truncate logic that used to live inline in `to_json`;
  `to_json` is now `serde_json::to_string(&self.into_report(matched))`.
- Computation always goes through the **gen-guarded background job**, never
  a synchronous full scan on the render thread — this holds even though the
  panel is opened at most once per keypress (not per-frame), because File
  `visible` can be file-scale (no eviction).
  - File mode: job clones `Arc<Mmap>` + `Arc<RwLock<Vec<LineSpan>>>` via
    `FileStore::scan_snapshot()` (same sharing model as
    `store.rs::spawn_filter_scan` — see `async-scans.md`), and parses on a
    background thread.
  - Stream mode: job clones the current `Vec<EntryRow>` snapshot before
    spawning (cheap one-shot `Clone`, not a lock held across the scan).
- **Gen check happens on message receipt in `poll_summary_job`, not by
  cancelling the thread.** A stale in-flight job's result is discarded, not
  aborted; this matches `CandidateMatchService`'s gen model
  (`candidate_match.rs`) rather than adding an `AtomicBool` cancel token.
- Bar charts are **hand-rolled Unicode block spans** (`theme::level_bar_style`
  / `theme::accent_bar_style` + `█`/`░` in a `Line`), never
  `ratatui::widgets::BarChart`/`Sparkline`/`Gauge` — keeps the summary panel
  on the same `Paragraph`/`List`/`Line`/`Span` rendering model as the rest
  of the TUI and lets colors route through `theme.rs`.
- Top errors list has **no** bar (pattern text is long; avoids double
  bar-chart clutter next to Top tags).

### 4. Validation & Error Matrix

| Condition | Behavior |
|-----------|----------|
| `Leader i` while `SummaryView::Closed` | `open_summary_panel()`; state → `Loading` until job result arrives |
| `Leader i` while `SummaryView::Ready`/`Loading` | Toggle: closes panel, bumps `summary_gen` |
| Job result arrives with stale `gen` | Discarded in `poll_summary_job`, no UI update |
| File mode, large `visible` (tested at 20k rows) | UI stays responsive; panel shows `Loading` until background job completes |
| `to_json` vs `into_report` on the same `Summary` | Identical field values (see `alnav-core/tests/summary.rs::test_into_report_matches_to_json`) |

### 5. Good/Base/Bad Cases

- **Good**: Open panel on a filtered File view, get level distribution +
  Top tags (bar) + Top errors (list) + crash count + time range once
  Loading resolves to Ready.
- **Base**: Close and immediately reopen — old job's late result is
  dropped by gen check, new job's result is shown.
- **Bad**: Adding a new `AtomicBool` cancel flag instead of reusing the
  `gen` discard-on-receipt pattern, or calling `Summary::record()`
  synchronously on the render thread for a multi-hundred-thousand-row File
  `visible`.

### 6. Tests Required

- `alnav-core::summary` — `into_report` output equals `to_json`'s parsed
  JSON for the same `Summary` instance.
- `alnav::app` — `open_summary_panel`/`poll_summary_job` gen-discard on
  rapid close→reopen; File async path over a non-trivial row count reaches
  `Ready`; Stream path reaches `Ready` with correct counts.

### 7. Wrong vs Correct

#### Wrong
```rust
// Synchronous full scan on keypress — blocks the render thread for
// file-scale `visible`
let report = build_summary_sync(&app.store, &app.visible);
app.summary_view = SummaryView::Ready(report);
```

#### Correct
```rust
app.open_summary_panel(); // bumps gen, spawns background job
// ...
app.poll_summary_job(); // called every frame; drops stale-gen results
```

---

## Scenario: Device Disconnect Indicator

### 1. Scope / Trigger

Update when changing `App.ingest_done` semantics or the status bar's
"live state" icon cluster.

### 2. Signatures

| Item | Location | Contract |
|------|----------|----------|
| `theme::GLYPH_DISCONNECT` | `theme.rs` | `\u{f127}` (nf-fa-chain-broken) |
| `render_status_bar` disconnect branch | `ui.rs` | `if !app.store.is_file() && app.ingest_done` |

### 3. Contracts

- **Reuses `App.ingest_done` — no new field.** That flag already flips to
  `true` for Stream mode when the live child's stdout iterator ends
  (`ingest.rs::spawn_live_ingest` → `mark_disconnected` → `drain()` sees
  `TryRecvKind::Disconnected`).
- **Must gate on `!app.store.is_file()`.** File mode also sets
  `ingest_done = true` on normal "finished reading the whole file", which
  is not a disconnect — showing the icon there would be a false positive
  on every static `-f` session.
- Icon only, no attached value (`theme::status_icon`, not
  `status_icon_value`) — matches `following`/`visual`'s boolean-state
  rendering, not `lock`/`time`'s "icon + short value" rendering.
- Color is `theme::warning()`; no new color constant.
- Position: after the `following` icon, before the `lock` badge (same
  "live state" cluster).
- Does not distinguish disconnect *cause* (exit code, device unplug vs.
  `adb`/`hdc` binary missing) — `ingest.rs` doesn't capture that
  information; out of scope for v1.
- No auto-reconnect — indicator only.

### 4. Validation & Error Matrix

| Condition | Behavior |
|-----------|----------|
| Stream mode, `ingest_done == true` | Icon shown |
| Stream mode, `ingest_done == false` | Icon hidden |
| File mode, `ingest_done == true` (finished reading) | Icon hidden (not a disconnect) |

### 5. Good/Base/Bad Cases

- **Good**: `--adb` child process exits (device unplugged) → icon appears,
  `following`/`lock`/`time` badges unaffected.
- **Bad**: Showing the icon whenever `ingest_done` is true without checking
  `store.is_file()` — every `-f` session would show a false "disconnected"
  badge right after the file finishes loading.

### 6. Tests Required

- `ui::` — Stream mode + `ingest_done=true` renders `GLYPH_DISCONNECT`;
  File mode + `ingest_done=true` does not.

---

## Scenario: Single-line/Multi-line Wrap Toggle (`w`)

### 1. Scope / Trigger

Update when changing `App.collapsed_view`, LogList row rendering, or the
Preview panel's single-line truncation algorithm (`render_entry_line_single`)
that this feature reuses.

### 2. Signatures

| Item | Location | Contract |
|------|----------|----------|
| `ActionId::LogListWrapToggle` | `keymap.rs` | `KeyContext::LogList`, `toml_key: "wrap_toggle"`, default `w` |
| `App.collapsed_view: bool` | `app.rs` | Session-only, default `false`, no `config.toml` persistence |
| `App::toggle_collapsed_view()` | `app.rs` | Flips `collapsed_view`; does not touch `cursor`/`following`/`list_offset` |
| `render_entry_line_collapsed` | `ui.rs` | Single-line renderer with lineno prefix; truncation algorithm mirrors `render_entry_line_single` (Preview) |
| `render_log_list` dispatch | `ui.rs` | `if app.collapsed_view { render_entry_line_collapsed } else { render_entry_lines }` |

### 3. Contracts

- **Global session toggle, not per-row expand/collapse.** One `bool` on
  `App` drives the whole `render_log_list` call, not a per-`EntryRow` state.
- **Never persisted to `config.toml`.** Resets to multi-line (`false`) on
  every new process — collapsed mode is a "scan fast right now" mode, not a
  durable preference (matches the project's "default multi-line display"
  design principle).
- **No hit-priority clipping.** When a highlight/search match falls inside
  the truncated (post-"…") part of a collapsed line, it is simply not
  visible — there is no "center truncation on the match" logic. Users
  switch back to multi-line (`w` again) or use Fields/Pretty to see the
  full match.
- Paging/scrolling (`PAGE_SIZE`, `move_cursor_manual`, fast-scroll, mouse
  wheel) is untouched — they already operate on `visible` row indices, not
  rendered line counts, so collapsing `ListItem` height from N lines to 1
  needs no special-casing.

### 4. Validation & Error Matrix

| Condition | Behavior |
|-----------|----------|
| `w` in LogList focus | Toggles `collapsed_view`; render updates same frame |
| Collapsed + msg wider than budget | Truncated with `…`; lineno/ts/level/tag prefix unchanged |
| Collapsed + highlight hit past truncation point | Hit not visually shown; not a bug per R4 |
| New process start | `collapsed_view` defaults to `false` (multi-line) |

### 5. Good/Base/Bad Cases

- **Good**: `w` toggles a long-message-heavy LogList between multi-line
  wrap and single-line `…` truncation without moving the cursor or
  breaking `following`.
- **Bad**: Adding per-row collapse state (tree-view style) instead of one
  global `App.collapsed_view` bool — out of scope, not requested.

### 6. Tests Required

- `app::` — `w` toggles `collapsed_view`, leaves `cursor`/`following`/
  `list_offset` untouched.
- `ui::` — collapsed rendering truncates long msg with `…`; short msg
  unaffected; `render_log_list` in collapsed mode produces single-`Line`
  `ListItem`s.

---

## Design Decision: Reuse Over New Mechanisms

**Context**: All four features could have been built as fully independent
subsystems (new keybindings, new cancel-token types, new persisted config,
new chart widgets).

**Decision**: Each one binds to an existing mechanism instead —
`P`'s existing JSON/raw fallback chain, `CandidateMatchService`'s gen-discard
pattern, `App.ingest_done`, and Preview's single-line truncation.

**Why**: Keeps the number of "ways to do X" in the TUI small (one detail-view
key, one async-job cancellation idiom, one disconnect signal, one truncation
algorithm), which is what `directory-structure.md`'s reuse-first guidance and
`CLAUDE.md`'s "跨 crate 的日志颜色统一" precedent both push toward.
