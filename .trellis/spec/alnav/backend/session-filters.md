# Session Filters (lock + global time window)

> Executable contracts for App-level AND filters that sit outside chip Groups.

---

## Scenario: Global time window (`App.time_bound`)

### 1. Scope / Trigger

- Trigger: interactive / startup time range filtering in TUI (`07-23-tui-time-window`).
- Cross-module: `filter_model::TimeBound`, `App`, `time_panel`, `export`, status bar, key dispatch.

### 2. Signatures

```rust
// filter_model.rs
pub struct TimeBound {
    pub since: Option<String>,
    pub until: Option<String>,
}
impl TimeBound {
    pub fn is_active(&self) -> bool; // either side Some
    pub fn matches(&self, entry: &LogEntry<'_>) -> bool;
}

// app.rs
pub time_bound: Option<TimeBound>;
pub pending_time: bool;
pub time_panel: Option<TimePanel>;

pub fn is_file_mode(&self) -> bool;
pub fn begin_time_op(&mut self);           // arm `t` pending
pub fn open_time_panel(&mut self) -> bool; // `ts`; false → flash 无可用日期
pub fn close_time_panel(&mut self);        // Esc; no apply; no resume_following
pub fn apply_time_bound(&mut self, bound: TimeBound);
pub fn clear_time_bound(&mut self);        // `tu`
pub fn time_badge_label(&self) -> Option<String>;
pub fn row_passes_filters(&self, row: &EntryRow) -> bool;
// order: groups.matches → lock pid/tid → time_bound.matches
pub fn filter_active(&self) -> bool;
// includes time_bound.is_some_and(TimeBound::is_active)

// time_panel.rs
pub fn TimePanel::open(rows: &VecDeque<EntryRow>, bound: Option<&TimeBound>) -> Option<Self>;
pub fn handle_key(&mut self, code: KeyCode) -> TimePanelOutcome;

// export.rs
pub fn build_cli_command(
    source: &ExportSource,
    groups: &GroupList,
    lock_pid: Option<&str>,
    lock_tid: Option<&str>,
    time_bound: Option<&TimeBound>,
) -> String;
```

### 3. Contracts

| Field / key | Rules |
|-------------|--------|
| Ownership | Time lives on `App.time_bound` only. **Never** on `Group`. |
| Startup CLI | `--since`/`--until` → `initial_time_bound()` → `App.time_bound`; `initial_group` has chips/expr only. |
| Mode gate | Interactive `t`/`ts`/`tu` only when `ExportSource::File`. `--hdc` and `--adb`: hard no-op (do not arm `pending_time`). |
| Date candidates | Dedup from current `rows` via `time_full()` date prefix (`MM-DD` or `YYYY-MM-DD`). Empty → refuse `ts`. |
| Date input | Typeahead filter; select only from candidates; no custom dates. |
| Time input | `HH:MM:SS`; normalize then clamp to that date’s min/max in buffer; both sides set → clamp **current** side so since ≤ until. |
| Partial sides | One side may be empty; a non-empty side must have date+time pair or flash `端内需日期+时间`. |
| Clear | `tu` only; empty Enter ≠ clear. |
| Following | open / apply / `tu` → `following=false`; panel Esc / Ctrl+C cancel → no `resume_following`. |
| Export | `yc` emits `--since`/`--until` from `App.time_bound`. |
| Status | `TIME …` badge via `theme::GLYPH_TIME` + `status_badge` (no raw `Color` in `ui.rs`). |

### 4. Validation & Error Matrix

| Condition | Behavior |
|-----------|----------|
| `ts` with empty date catalog | Flash `无可用日期`; panel stays closed |
| Submit with partial side | Flash `端内需日期+时间`; panel stays open |
| Submit with both sides empty | Flash `未设置时间窗`; panel stays open |
| `tu` with no active bound | Flash `无时间窗` |
| `tu` with active bound | Clear, `rebuild_visible`, flash `TIME CLEARED` |
| live source + `t` | No pending; key ignored (file-mode match arm) |
| Panel Ctrl+C | Same as Esc cancel (must not insert `c` into draft) |

### 5. Good / Base / Bad Cases

- **Good**: `-f` log with dates → `ts` → select date → type HMS → Enter through fields → `TIME` badge + filtered `visible`; `yc` contains `--since`.
- **Base**: Startup `alnav -f f --since 10:00:00` → `App.time_bound` set, groups empty or chip-only; `di` on group 0 does not clear time.
- **Bad**: Putting `time` back on `Group` so `di` disables the window; opening `ts` under `--hdc` or `--adb`; allowing typed dates outside candidates.

### 6. Tests Required

- `filter_model` / `TimeBound::matches` (HMS + full datetime)
- `time_panel`: catalog extract, normalize/clamp, one-sided submit, partial flash, prefill
- `app`: `filter_active` with time only; `di` does not clear `time_bound`; badge label
- `export`: global bound → `--since`/`--until`
- `main` dispatch: `ts`/`tu`, empty catalog flash, both live backends hard-hide, panel Ctrl+C cancel
- `help`: `pending_time` → L2_TIME

Assertion points: `Group` has no `time` field; `row_passes_filters` ANDs time after lock.

### 7. Wrong vs Correct

#### Wrong
```rust
// Time on Group — di/dd can disable the window accidentally
groups.groups[0].time = Some(bound);
```

#### Correct
```rust
app.time_bound = Some(bound);
// matching:
if !self.groups.matches(row) { return false; }
// … lock …
if let Some(bound) = &self.time_bound {
    if !bound.matches(&row.as_log_entry()) { return false; }
}
```

---

## Design Decision: Global window vs Group.time

**Context**: CLI `--since`/`--until` are global AND; interactive time must match that mental model and not be toggled by Filter `di`.

**Decision**: `App.time_bound` only; remove `Group.time`.

**Extensibility**: Cursor-derived set / relative windows can write the same `App.time_bound` without reviving Group.time.
