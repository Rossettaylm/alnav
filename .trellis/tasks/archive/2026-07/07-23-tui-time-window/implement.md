# Implement: TUI global time window

## Checklist

1. **Model migrate**
   - Add `App.time_bound` / `pending_time` / panel slot.
   - Remove `Group.time`; update `Group::matches` / `same_as` / filter_model tests.
   - `initial_group`: chips/expr only; return time separately or set on `App` in `main`.
   - `row_passes_filters` + `filter_active` + `export_cli_command`.

2. **TimePanel module**
   - Date candidates + per-date min/max from `&[EntryRow]` / `VecDeque`.
   - Prefill from `TimeBound` strings (best-effort).
   - Clamp HMS + since≤until on current side.
   - Submit → `Option<TimeBound>` (omit incomplete sides; reject if both incomplete or a side partial).

3. **Keys + lifecycle**
   - File-mode only `t`/`ts`/`tu`; hdc no-op.
   - Empty candidates flash; following rules per PRD.
   - help.rs L2 for pending_time.

4. **UI**
   - Top modal panel render; status TIME badge via theme.

5. **Docs**
   - CLAUDE.md: global time window + keys; drop interactive-time YAGNI.

6. **Tests**
   - TimeBound still unit-tested; App filter_active with time only; export; candidate extract; clamp; initial_group time not on group; key dispatch smoke if existing patterns allow.

## Validation

```bash
cargo test -p aloggrep-tui filter_model::
cargo test -p aloggrep-tui time_panel::
cargo test -p aloggrep-tui app::
cargo test -p aloggrep-tui export::
cargo test -p aloggrep-tui
```

Manual (TTY, `-f` sample with dates):
- `ts` → pick date → type time → Tab through → Enter → TIME badge + filtered list
- one-sided since only; partial side flash
- `tu` clears
- `--hdc`：`ts`/`tu` 无效
- `-f --since/--until` startup still filters; `di` group 0 does not clear time

## Risky files

- `main.rs` key dispatch (collision with future bare `t`)
- `filter_model.rs` Group shape change
- `export.rs` API signature

## Before `task.py start`

- [x] User approved grilling shared understanding +「新建任务并执行」
- [x] `prd.md` / `design.md` / `implement.md` present
- [x] Curate `implement.jsonl` / `check.jsonl`
