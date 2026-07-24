# Design: TUI global time window

## Architecture

```
CLI --since/--until ──► App.time_bound: Option<TimeBound>
                              │
ts panel (date candidates + HH:MM:SS) ──┘
                              │
row_passes_filters: groups → lock → time_bound.matches(entry)
                              │
filter_active |= time_bound.is_some()
export / status TIME badge / yc
```

Reuse existing [`TimeBound`](aloggrep-tui/src/filter_model.rs) matching logic (`time_hms` / `time_full`). Move ownership from `Group.time` to `App`.

## Data model

### `App`
- `time_bound: Option<TimeBound>` — session global window (`since`/`until` optional strings).
- `pending_time: bool` — operator-pending after `t` (mirror `pending_lock`).
- `time_panel: Option<TimePanel>` — open editor state; `None` when closed.
- File-mode gate: `matches!(export_source, ExportSource::File(_))`.

### Remove / stop using `Group.time`
- `initial_group`: chips+expr only; if only time flags present and no chips/expr → empty `GroupList`, time goes to `App.time_bound`.
- `Group::matches` / `same_as`: drop time (or leave field always `None` and delete in same change — prefer **remove `time` field** to avoid dual path).
- Tests that built time-only groups update to set `App.time_bound`.

### `TimePanel` (new, prefer `aloggrep-tui/src/time_panel.rs`)
Fields (conceptual):
- focus index 0..3: since_date, since_time, until_date, until_time
- each side: `selected_date: Option<String>`, date query string, candidate list + highlight
- each side: time draft `HH:MM:SS` (or partial while typing)
- on open: build date candidates from `app.rows`; prefill from `app.time_bound` via best-effort split

## Date candidates

From `app.rows` only:
- For each row with `as_log_entry().time_full()`:
  - xlog: date = first 10 chars `YYYY-MM-DD`
  - hilog/threadtime: date = first 5 chars `MM-DD`
- Dedup, sort ascending.
- Per-date min/max `HH:MM:SS` from rows on that date (for clamp).

Empty candidates → `ts` flash, do not open.

## Time clamp (submit / leave time field)

1. Parse / normalize to `HH:MM:SS` (invalid components → clamp 0–23 / 0–59).
2. If side has selected date: clamp HMS into that date’s buffer min/max.
3. If both sides complete: compare full datetime strings compatible with `TimeBound` storage; if since > until, clamp **the field being committed** (current side).

Stored bound after panel submit: compose `"{date} {hms}"` using the candidate date form (so `time_full` path applies), unless product later needs time-only — panel always has date when a side is set.

## Key dispatch (`main.rs`)

Mirror lock operator:
- LogList + file mode: `t` → `pending_time = true` (help L2: `s:设窗 u:清除`).
- `ts` → if candidates empty flash; else `following=false`, open `TimePanel`.
- `tu` → clear `time_bound`, `following=false`, `rebuild_visible`.
- Esc while pending_time → clear pending only (no resume).
- `--hdc` / non-file: do not arm `pending_time` (ignore `t` as today if unused, or no-op).

When `time_panel` open: route keys to panel; Esc closes without apply; do not resume following.

## Matching / export / UI

- `App::row_passes_filters`: after lock, if `time_bound` then `time_bound.matches(&row.as_log_entry())`.
- `filter_active`: OR `self.time_bound.as_ref().map_or(false, |t| t.since.is_some() || t.until.is_some())`.
- `export::build_cli_command`: take `time_bound: Option<&TimeBound>` (or since/until args) instead of `shared_time_bound(groups)`.
- Status bar: `TIME` badge via `theme` tokens (reuse LOCK-like badge helper; add glyph/label helper if needed — **no raw Color in ui.rs**).
- Help: L2 for `pending_time`.

## Rendering

- Reuse top modal shell (`render_modal_shell` / `top_modal_rect`) for the panel.
- Left/right or stacked since/until blocks; date field shows filtered candidate list under the field (picker-like, not full PickerSession).
- Time field shows draft + caret.

## Preview

MVP: no live Preview sampling required (YAGNI unless cheap to hook). Filtering applies on submit.

## Files to touch

| File | Change |
|------|--------|
| `filter_model.rs` | Keep `TimeBound`; remove `Group.time` (+ tests) |
| `app.rs` | `time_bound`, pending, panel hooks, `row_passes_filters`, `filter_active`, clear/set APIs |
| `time_panel.rs` | **new** panel state + candidate/clamp/submit |
| `main.rs` | `initial_group` split time; `t`/`ts`/`tu`; panel key routing; wire `App.time_bound` from CLI |
| `ui.rs` | render panel + TIME badge |
| `export.rs` | export from global bound |
| `help.rs` | pending_time hints |
| `theme.rs` | TIME badge style if not reusing LOCK |
| `CLAUDE.md` | document feature; remove YAGNI line about interactive time chips |

## Risks

- Removing `Group.time` breaks tests / export helpers — update in same PR.
- Date form mix (MM-DD vs YYYY-MM-DD) in one file is rare; candidates keep native form; comparison uses existing `TimeBound` rules.
- Large `rows` candidate scan each `ts` open: O(n) acceptable for MVP (same order as rebuild).
